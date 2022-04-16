use crate::state::{get_seed, ContractData, ContractType};
use solana_program::{program_error::ProgramError, pubkey::Pubkey};
use std::convert::TryInto;

#[derive(Debug)]
pub enum InstructionType {
    /*
        expected accounts:
          buyer          [writable]
          prem_temp      [writable]
          receive_acc    [writable]
          receive_ata    [writable]
          mint_pda       [writable]
          holder_mint    [writable]
          data_pda       [writable]
          system_program []
          token_program  []
    */
    Bid { instruction: OfferData },
    /*
        expected accounts:
          writer              [writable]
          asset_temp          [writable]
          receive_acc         [writable]
          receive_ata         [writable]
          mint_pda            [writable]
          holder_mint         [writable]
          data_pda            [writable]
          system_program      []
          token_program       []
    */
    Ask { instruction: OfferData },
    /*
        expected accounts:
          writer              [writable, signer]
          asset_temp          [writable]
          strike_receive_acc  [writable]
          prem_receive_acc    [writable]
          data_pda            [writable]
          premium_temp        [writable] (owned by PDA)
          buyer               [writable]
          buyer_holder_ata    [writable]
          mint_pda            [writable]
          holder_mint         [writable]
          system_program      []
          token_program       []
    */
    AcceptBid,
    /*
        expected accounts:
          buyer            [writable]
          prem_temp        [writable]
          buyer_receive    [writable]
          holder_ata       [writable]
          mint_pda         [writable]
          holder_mint      [writable]
          data_pda         [writable]
          prem_receive_acc []
          system_program   []
          token_program    []
    */
    AcceptAsk,
    /*
        expected accounts:
          initialiser      [writable] (signer)
          token_temp       [writable] (owned by PDA)
          data_pda         [writable]
          mint_account     [writable]
          system_program   []
          token_program    []
    */
    CancelOffer,
    /*
        expected accounts:
          buyer             [writable] (signer)
          strike_temp       [writable]
          buyer_receive     [writable]
          buyer_holder_ata  [writable]
          mint_pda          [writable]
          buyer_holder_mint [writable]
          asset_temp        [writable] (owned by PDA)
          data_pda          [writable]
          writer            []
          writer_receive    []
          system_program    []
          token_program     []
    */
    Execute,
    /*
        expected accounts:
          writer         [writable] (signer)
          asset_temp     [writable] (owned by PDA)
          data_pda       [writable]
          system_program []
          token_program  []
    */
    Expire,
    /*
        expected accounts:
          sender         [writable, signer]
          holder_mint    [writable, signer] (not created)
          mint_pda       [writable] (not created)
          system_program []
          token_program  []
    */
    CreateMint { seeds: (u8, [u8; 32]) },
}

#[derive(Debug, PartialEq)]
pub enum InitParty {
    BUYER,
    WRITER,
}

#[derive(Debug)]
pub struct OfferData {
    pub contract_data: ContractData,
    pub pda: Pubkey,
    pub bump: u8,
    pub seed: [u8; 32],
    pub party: InitParty,
    pub contract_type: ContractType,
    pub index_seed: [u8; 32],
}

pub fn decode_instruction(
    program_id: &Pubkey,
    instruction_data: &[u8],
) -> Result<InstructionType, ProgramError> {
    match instruction_data[0] {
        0 => build_offer_data(program_id, InitParty::BUYER, instruction_data),
        1 => build_offer_data(program_id, InitParty::WRITER, instruction_data),
        2 => Ok(InstructionType::AcceptBid),
        3 => Ok(InstructionType::AcceptAsk),
        4 => Ok(InstructionType::CancelOffer),
        5 => Ok(InstructionType::Execute),
        6 => Ok(InstructionType::Expire),
        7 => Ok(InstructionType::CreateMint {
            seeds: (
                instruction_data[1],
                instruction_data[2..]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidInstructionData)?,
            ),
        }),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

// instruction data: [instruction_type, contract_type, ..contract_data, ..index_seed]
// index_seed format: [0..32 = initialiser main pubkey, 32 = contract_type, 33..41 = contract_no (u64)]

fn build_offer_data(
    pid: &Pubkey,
    party: InitParty,
    instruction_data: &[u8],
) -> Result<InstructionType, ProgramError> {
    let contract_type = match instruction_data[1] {
        0 => ContractType::CALL,
        1 => ContractType::PUT,
        _ => return Err(ProgramError::InvalidInstructionData),
    };
    let seed: [u8; ContractData::LEN] = instruction_data[2..ContractData::LEN + 2]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    let index_seed: [u8; 41] = instruction_data[ContractData::LEN + 2..]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let contract_data = ContractData::deserialize(&seed);

    let seed = get_seed(&seed);
    let index_seed = get_seed(&index_seed);

    let (pda, bump) = Pubkey::find_program_address(&[&seed, &index_seed], pid);

    let od = OfferData {
        contract_data,
        pda,
        bump,
        seed,
        index_seed,
        party,
        contract_type,
    };

    Ok(InstructionType::Bid { instruction: od })
}
