use crate::instruction::InitParty;
use arrayref::{array_refs, mut_array_refs};
use sha2::{Digest, Sha256};
use solana_program::{
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::Pubkey,
};
use std::convert::TryInto;

#[derive(Debug, PartialEq)]
pub struct ContractPDA {
    pub contract_data: ContractData,
    pub contract_state: ContractState,
    pub buyer_data: Option<PartyData>,
    pub writer_data: Option<PartyData>,
    pub is_initialised: bool,
    pub seed: [u8; 32],
    pub index_seed: [u8; 32],
    pub bump: u8,
    pub init_party: InitParty,
    pub contract_type: ContractType,
}

#[derive(Debug, PartialEq)]
pub struct MintPDA {
    pub holder_mint: Pubkey,
}
impl Sealed for MintPDA {}

impl Pack for MintPDA {
    const LEN: usize = 32;

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let src: &[u8; 32] = src
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        let holder_mint = Pubkey::new(&src[..32]);

        Ok(MintPDA { holder_mint })
    }

    fn pack_into_slice(&self, dst: &mut [u8]) {
        dst[..32].copy_from_slice(&self.holder_mint.to_bytes()[..]);
    }
}

impl Sealed for ContractPDA {}

impl IsInitialized for ContractPDA {
    fn is_initialized(&self) -> bool {
        self.is_initialised
    }
}

impl Pack for ContractPDA {
    const LEN: usize = 421;

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        if src.len() != ContractPDA::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let src: &[u8; ContractPDA::LEN] = src
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        let (
            is_initialised,
            seed,
            contract_state,
            buyer_data,
            writer_data,
            bump,
            init_party,
            contract_type,
            index_seed,
        ) = array_refs![src, 1, 128, 1, 128, 128, 1, 1, 1, 32];
        let bump = bump[0];

        let is_initialised = match is_initialised[0] {
            0 => false,
            1 => true,
            _ => return Err(ProgramError::InvalidAccountData),
        };

        let init_party = match init_party[0] {
            0 => InitParty::BUYER,
            1 => InitParty::WRITER,
            _ => return Err(ProgramError::InvalidAccountData),
        };

        let contract_type = match contract_type[0] {
            0 => ContractType::CALL,
            1 => ContractType::PUT,
            _ => return Err(ProgramError::InvalidAccountData),
        };

        let (contract_state, buyer_data, writer_data) = match contract_state[0] {
            0 => (
                ContractState::BID,
                Some(PartyData::from_bytes(buyer_data)),
                None,
            ),
            1 => (
                ContractState::ASK,
                None,
                Some(PartyData::from_bytes(writer_data)),
            ),
            2 => (
                ContractState::FINAL,
                Some(PartyData::from_bytes(buyer_data)),
                Some(PartyData::from_bytes(writer_data)),
            ),
            _ => return Err(ProgramError::InvalidAccountData),
        };

        let contract_data = ContractData::deserialize(seed);

        let seed = contract_data.get_seed();

        Ok(ContractPDA {
            is_initialised,
            contract_data,
            contract_state,
            buyer_data,
            writer_data,
            seed,
            bump,
            init_party,
            contract_type,
            index_seed: *index_seed,
        })
    }

    fn pack_into_slice(&self, dst: &mut [u8]) {
        let dst: &mut [u8; ContractPDA::LEN] = dst.try_into().unwrap();
        let (
            is_initialised,
            contract_data,
            contract_state,
            buyer_data,
            writer_data,
            bump,
            init_party,
            contract_type,
            index_seed,
        ) = mut_array_refs![dst, 1, 128, 1, 128, 128, 1, 1, 1, 32];

        is_initialised[0] = match self.is_initialised {
            true => 1,
            false => 0,
        };

        init_party[0] = match self.init_party {
            InitParty::WRITER => 1,
            InitParty::BUYER => 0,
        };

        contract_state[0] = match self.contract_state {
            ContractState::BID => 0,
            ContractState::ASK => 1,
            ContractState::FINAL => 2,
        };

        contract_type[0] = match self.contract_type {
            ContractType::CALL => 0,
            ContractType::PUT => 1,
        };

        contract_data.copy_from_slice(&self.contract_data.serialize());
        bump.copy_from_slice(&[self.bump]);
        index_seed.copy_from_slice(&self.index_seed);

        match &self.buyer_data {
            Some(bd) => {
                buyer_data.copy_from_slice(&bd.to_bytes());
            }
            None => {
                buyer_data.copy_from_slice(&[0; 128]);
            }
        };

        match &self.writer_data {
            Some(wd) => writer_data.copy_from_slice(&wd.to_bytes()),
            None => writer_data.copy_from_slice(&[0; 128]),
        };
    }
}

pub fn get_seed(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let seed: [u8; 32] = hasher.finalize().try_into().unwrap();
    seed
}

#[derive(Debug, PartialEq)]
pub struct PartyData {
    pub party_pub: Pubkey,
    pub temp_pub: Pubkey,
    pub receive_pub: Pubkey,
    pub receive_ata: Pubkey,
}
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ContractType {
    CALL,
    PUT,
}

impl PartyData {
    pub fn from_bytes(bytes: &[u8; 128]) -> Self {
        PartyData {
            party_pub: Pubkey::new_from_array(
                bytes[0..32]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidAccountData)
                    .unwrap(),
            ),
            temp_pub: Pubkey::new_from_array(
                bytes[32..64]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidAccountData)
                    .unwrap(),
            ),
            receive_pub: Pubkey::new_from_array(
                bytes[64..96]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidAccountData)
                    .unwrap(),
            ),
            receive_ata: Pubkey::new_from_array(
                bytes[96..]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidAccountData)
                    .unwrap(),
            ),
        }
    }
    pub fn to_bytes(&self) -> [u8; 128] {
        [
            self.party_pub.to_bytes(),
            self.temp_pub.to_bytes(),
            self.receive_pub.to_bytes(),
            self.receive_ata.to_bytes(),
        ]
        .concat()
        .try_into()
        .unwrap()
    }
}

#[derive(Debug, PartialEq)]
pub enum ContractState {
    BID,
    ASK,
    FINAL,
}

#[derive(Debug, PartialEq)]
pub struct ContractData {
    pub token_type: Pubkey,
    pub token_qty: u64,
    pub expiry_date: i64,
    pub strike_type: Pubkey,
    pub strike_qty: u64,
    pub premium_type: Pubkey,
    pub premium_qty: u64,
}

impl ContractData {
    pub const LEN: usize = 128;
    pub fn deserialize(data_array: &[u8]) -> ContractData {
        let data_array: &[u8; Self::LEN] = data_array
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)
            .unwrap();
        let (
            token_type,
            token_qty,
            expiry_date,
            strike_type,
            strike_qty,
            premium_type,
            premium_qty,
        ) = array_refs![data_array, 32, 8, 8, 32, 8, 32, 8];

        let token_type = Pubkey::new_from_array(*token_type);
        let token_qty = u64::from_le_bytes(*token_qty);
        let expiry_date = i64::from_le_bytes(*expiry_date);
        let strike_type = Pubkey::new_from_array(*strike_type);
        let strike_qty = u64::from_le_bytes(*strike_qty);
        let premium_type = Pubkey::new_from_array(*premium_type);
        let premium_qty = u64::from_le_bytes(*premium_qty);

        ContractData {
            token_type,
            token_qty,
            expiry_date,
            strike_type,
            strike_qty,
            premium_type,
            premium_qty,
        }
    }

    pub fn serialize(&self) -> [u8; Self::LEN] {
        let mut v = Vec::with_capacity(Self::LEN);
        v.extend_from_slice(&self.token_type.to_bytes());
        v.extend_from_slice(&self.token_qty.to_le_bytes());
        v.extend_from_slice(&self.expiry_date.to_le_bytes());
        v.extend_from_slice(&self.strike_type.to_bytes());
        v.extend_from_slice(&self.strike_qty.to_le_bytes());
        v.extend_from_slice(&self.premium_type.to_bytes());
        v.extend_from_slice(&self.premium_qty.to_le_bytes());
        v.try_into().unwrap()
    }

    pub fn get_seed(&self) -> [u8; 32] {
        let mut dst = [0; 120];
        dst[0..32].copy_from_slice(&self.token_type.to_bytes());
        dst[32..40].copy_from_slice(&self.token_qty.to_le_bytes());
        dst[40..48].copy_from_slice(&self.expiry_date.to_le_bytes());
        dst[48..80].copy_from_slice(&self.strike_type.to_bytes());
        dst[80..88].copy_from_slice(&self.strike_qty.to_le_bytes());
        dst[88..120].copy_from_slice(&self.premium_type.to_bytes());

        get_seed(&dst)
    }
}
