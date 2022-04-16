use crate::instruction;
use crate::state::{get_seed, ContractPDA, ContractState, ContractType::*, MintPDA, PartyData};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::{Clock, SLOT_MS},
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    system_instruction, system_program,
    sysvar::{rent, Sysvar},
};
use spl_associated_token_account::get_associated_token_address;
use spl_token;

pub fn initialise_contract(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction: instruction::OfferData,
) -> Result<(), ProgramError> {
    let instruction::OfferData {
        contract_data,
        pda,
        bump,
        seed,
        party,
        contract_type,
        index_seed,
    } = instruction;

    let min_rent = rent::Rent::get()?.minimum_balance(ContractPDA::LEN);

    let accounts = &mut accounts.iter();

    let initialiser = next_account_info(accounts)?;
    let token_temp = next_account_info(accounts)?;
    let receive_acc = next_account_info(accounts)?;
    let receive_ata = next_account_info(accounts)?;
    let mint_pda = next_account_info(accounts)?;
    let holder_mint = next_account_info(accounts)?;
    let data_pda = next_account_info(accounts)?;
    let sys_program = next_account_info(accounts)?;
    let token_program = next_account_info(accounts)?;

    let token_temp_info =
        spl_token::state::Account::unpack_from_slice(*token_temp.try_borrow_data()?)?;
    let rec_account_info =
        spl_token::state::Account::unpack_from_slice(*receive_acc.try_borrow_data()?)?;
    let receive_ata_info =
        spl_token::state::Account::unpack_from_slice(*receive_ata.try_borrow_data()?)?;

    let mint_pda_data = MintPDA::unpack_from_slice(*mint_pda.try_borrow_data()?)?;

    msg!("asserting validity...");
    if !system_program::check_id(sys_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !spl_token::check_id(token_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if *data_pda.key != pda {
        msg!("INCORRECT PDA ACCOUNT");
        return Err(ProgramError::IncorrectProgramId);
    }
    if !initialiser.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !data_pda.try_data_is_empty()? {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    let (temp_type, temp_qty, rec_type) = match party {
        instruction::InitParty::BUYER => (
            contract_data.premium_type,
            contract_data.premium_qty,
            match contract_type {
                CALL => contract_data.token_type,
                PUT => contract_data.strike_type,
            },
        ),
        instruction::InitParty::WRITER => match contract_type {
            CALL => (
                contract_data.token_type,
                contract_data.token_qty,
                contract_data.strike_type,
            ),
            PUT => (
                contract_data.strike_type,
                contract_data.strike_qty,
                contract_data.token_type,
            ),
        },
    };

    if token_temp_info.mint != temp_type {
        msg!("INCORRECT TEMP ACCOUNT TYPE");
        return Err(ProgramError::InvalidArgument);
    };
    if token_temp_info.amount != temp_qty {
        msg!("INCORRECT TEMP ACCOUNT BALANCE");
        return Err(ProgramError::InvalidArgument);
    }
    if rec_account_info.mint != rec_type {
        msg!("INCORRECT RECEIVE ACCOUNT TYPE");
        return Err(ProgramError::InvalidArgument);
    }
    match party {
        instruction::InitParty::WRITER => {
            if receive_ata_info.mint != contract_data.premium_type {
                msg!("INCORRECT PREMIUM RECEIVE ACCOUNT TYPE");
                return Err(ProgramError::InvalidInstructionData);
            };
        }
        instruction::InitParty::BUYER => {
            let s1 = match contract_type {
                CALL => &[0],
                PUT => &[1],
            };
            let mint_seed = get_seed(&contract_data.serialize());
            let (mint_pdak, _bump) = Pubkey::find_program_address(&[s1, &mint_seed], program_id);
            if mint_pdak != *mint_pda.key {
                msg!("INCORRECT MINT PDA ACCOUNT");
                return Err(ProgramError::InvalidArgument);
            }
            if *holder_mint.key != mint_pda_data.holder_mint {
                msg!("INCORRECT HOLDER MINT ACCOUNT");
                return Err(ProgramError::InvalidArgument);
            }
            let x_ata = get_associated_token_address(initialiser.key, holder_mint.key);
            if x_ata != *receive_ata.key {
                msg!("INCORRECT HOLDER MINT ATA");
                return Err(ProgramError::InvalidArgument);
            }
        }
    };

    msg!("building PDA data...");
    let pda_data = match party {
        instruction::InitParty::BUYER => ContractPDA {
            contract_data,
            contract_state: ContractState::BID,
            buyer_data: Some(PartyData {
                party_pub: initialiser.key.clone(),
                temp_pub: token_temp.key.clone(),
                receive_pub: receive_acc.key.clone(),
                receive_ata: receive_ata.key.clone(),
            }),
            writer_data: None,
            is_initialised: true,
            bump,
            seed,
            init_party: instruction::InitParty::BUYER,
            contract_type,
            index_seed,
        },
        instruction::InitParty::WRITER => ContractPDA {
            contract_data,
            contract_state: ContractState::ASK,
            buyer_data: None,
            writer_data: Some(PartyData {
                party_pub: initialiser.key.clone(),
                temp_pub: token_temp.key.clone(),
                receive_pub: receive_acc.key.clone(),
                receive_ata: receive_ata.key.clone(),
            }),
            is_initialised: true,
            bump,
            seed,
            init_party: instruction::InitParty::WRITER,
            contract_type,
            index_seed,
        },
    };

    msg!("creating PDA...");
    let create_pda = system_instruction::create_account(
        &initialiser.key,
        &pda,
        min_rent,
        ContractPDA::LEN as u64,
        program_id,
    );

    invoke_signed(
        &create_pda,
        &[initialiser.clone(), data_pda.clone(), sys_program.clone()],
        &[&[&seed, &index_seed, &[bump]]],
    )?;

    msg!("transferring temp ownership to PDA...");
    let transfer_temp = spl_token::instruction::set_authority(
        token_program.key,
        token_temp.key,
        Some(&pda),
        spl_token::instruction::AuthorityType::AccountOwner,
        initialiser.key,
        &[initialiser.key],
    )?;

    invoke(
        &transfer_temp,
        &[
            token_temp.clone(),
            initialiser.clone(),
            token_program.clone(),
        ],
    )?;
    msg!("updating PDA data...");
    pda_data.pack_into_slice(*data_pda.data.borrow_mut());
    Ok(())
}

pub fn accept_bid(program_id: &Pubkey, accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    let accounts = &mut accounts.iter();

    let writer = next_account_info(accounts)?;
    let writer_temp = next_account_info(accounts)?;
    let writer_receive = next_account_info(accounts)?;
    let receive_ata = next_account_info(accounts)?;
    let data_pda = next_account_info(accounts)?;
    let premium_temp = next_account_info(accounts)?;
    let buyer = next_account_info(accounts)?;
    let buyer_holder_ata = next_account_info(accounts)?;
    let mint_pda = next_account_info(accounts)?;
    let holder_mint = next_account_info(accounts)?;
    let sys_program = next_account_info(accounts)?;
    let token_program = next_account_info(accounts)?;

    if data_pda.try_data_is_empty()? {
        return Err(ProgramError::InvalidAccountData);
    }

    let writer_temp_info =
        spl_token::state::Account::unpack_from_slice(*writer_temp.try_borrow_data()?)?;
    let writer_receive_info =
        spl_token::state::Account::unpack_from_slice(*writer_receive.try_borrow_data()?)?;
    let receive_ata_info =
        spl_token::state::Account::unpack_from_slice(*writer_receive.try_borrow_data()?)?;
    let premium_temp_info =
        spl_token::state::Account::unpack_from_slice(*premium_temp.try_borrow_data()?)?;
    let mut contract_pda = ContractPDA::unpack_from_slice(*data_pda.try_borrow_data()?)?;
    let mint_pda_data = MintPDA::unpack_from_slice(*mint_pda.try_borrow_data()?)?;

    let clock = Clock::get()?;
    let time = (clock.slot * SLOT_MS) as i64 + (clock.unix_timestamp * 1000);

    let bd = contract_pda.buyer_data.unwrap();

    let s1 = match contract_pda.contract_type {
        CALL => &[0],
        PUT => &[1],
    };
    let mint_seed = get_seed(&contract_pda.contract_data.serialize());
    let (mint_pdak, mint_bump) = Pubkey::find_program_address(&[s1, &mint_seed], program_id);

    msg!("unpacked accounts, asserting validity...");
    match contract_pda.contract_state {
        ContractState::BID => (),
        _ => {
            msg!("INVALID CONTRACT STATE");
            return Err(ProgramError::InvalidArgument);
        }
    };
    if time > contract_pda.contract_data.expiry_date {
        msg!("CONTRACT HAS EXPIRED");
        return Err(ProgramError::InvalidArgument);
    }
    if !system_program::check_id(sys_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !spl_token::check_id(token_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !writer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let (temp_type, temp_qty, rec_type) = match contract_pda.contract_type {
        CALL => (
            contract_pda.contract_data.token_type,
            contract_pda.contract_data.token_qty,
            contract_pda.contract_data.strike_type,
        ),
        PUT => (
            contract_pda.contract_data.strike_type,
            contract_pda.contract_data.strike_qty,
            contract_pda.contract_data.token_type,
        ),
    };
    if writer_temp_info.mint != temp_type {
        msg!("INCORRECT WRITER_TEMP TOKEN TYPE");
        return Err(ProgramError::InvalidArgument);
    }
    if writer_temp_info.amount != temp_qty {
        msg!("INCORRECT ASSET_TEMP BALANCE");
        return Err(ProgramError::InvalidArgument);
    }
    if writer_receive_info.mint != rec_type {
        msg!("INCORRECT RECEIVE_ACCOUNT TOKEN TYPE");
        return Err(ProgramError::InvalidArgument);
    }
    if *premium_temp.key != bd.temp_pub {
        msg!("INCORRECT PREMIUM_TEMP ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if receive_ata_info.mint != contract_pda.contract_data.premium_type {
        msg!("INCORRECT PREMIUM_RECEIVE ACCOUNT TOKEN TYPE");
        return Err(ProgramError::InvalidArgument);
    }
    if *buyer.key != bd.party_pub {
        msg!("INCORRECT BUYER ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if *buyer_holder_ata.key != bd.receive_ata {
        msg!("INCORRECT BUYER HOLDER ATA");
        return Err(ProgramError::InvalidArgument);
    }
    if *holder_mint.key != mint_pda_data.holder_mint {
        msg!("INCORRECT HOLDER MINT ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if *mint_pda.key != mint_pdak {
        msg!("INCORRECT MINT PDA ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }

    msg!("transferring writer_temp ownership to PDA...");
    let transfer_temp = spl_token::instruction::set_authority(
        token_program.key,
        writer_temp.key,
        Some(data_pda.key),
        spl_token::instruction::AuthorityType::AccountOwner,
        writer.key,
        &[writer.key],
    )?;

    invoke(
        &transfer_temp,
        &[writer_temp.clone(), writer.clone(), token_program.clone()],
    )?;

    msg!("transferring premium to writer...");
    let transfer_prem = spl_token::instruction::transfer(
        token_program.key,
        premium_temp.key,
        receive_ata.key,
        data_pda.key,
        &[data_pda.key],
        premium_temp_info.amount,
    )?;

    invoke_signed(
        &transfer_prem,
        &[
            premium_temp.clone(),
            receive_ata.clone(),
            data_pda.clone(),
            token_program.clone(),
        ],
        &[&[
            &contract_pda.seed,
            &contract_pda.index_seed,
            &[contract_pda.bump],
        ]],
    )?;

    msg!("closing premium temp account...");
    let close_prem_temp = spl_token::instruction::close_account(
        token_program.key,
        premium_temp.key,
        buyer.key,
        data_pda.key,
        &[data_pda.key],
    )?;

    invoke_signed(
        &close_prem_temp,
        &[
            premium_temp.clone(),
            buyer.clone(),
            data_pda.clone(),
            token_program.clone(),
        ],
        &[&[
            &contract_pda.seed,
            &contract_pda.index_seed,
            &[contract_pda.bump],
        ]],
    )?;

    msg!("minting holder_mint token");
    let ix = spl_token::instruction::mint_to(
        token_program.key,
        holder_mint.key,
        buyer_holder_ata.key,
        mint_pda.key,
        &[mint_pda.key],
        1,
    )?;
    invoke_signed(
        &ix,
        &[
            holder_mint.clone(),
            buyer_holder_ata.clone(),
            mint_pda.clone(),
        ],
        &[&[s1, &mint_seed, &[mint_bump]]],
    )?;

    msg!("updating PDA data...");
    contract_pda.contract_state = ContractState::FINAL;
    contract_pda.writer_data = Some(PartyData {
        party_pub: writer.key.clone(),
        temp_pub: writer_temp.key.clone(),
        receive_pub: writer_receive.key.clone(),
        receive_ata: receive_ata.key.clone(),
    });
    contract_pda.buyer_data = Some(bd);

    contract_pda.pack_into_slice(*data_pda.data.borrow_mut());
    Ok(())
}

pub fn accept_ask(program_id: &Pubkey, accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    let accounts = &mut accounts.iter();
    let buyer = next_account_info(accounts)?;
    let premium_temp = next_account_info(accounts)?;
    let buyer_receive = next_account_info(accounts)?;
    let holder_ata = next_account_info(accounts)?;
    let mint_pda = next_account_info(accounts)?;
    let holder_mint = next_account_info(accounts)?;
    let data_pda = next_account_info(accounts)?;
    let seller_prem_acc = next_account_info(accounts)?;
    let sys_program = next_account_info(accounts)?;
    let token_program = next_account_info(accounts)?;

    let mut contract_pda = ContractPDA::unpack_from_slice(*data_pda.try_borrow_data()?)?;

    let prem_temp_info =
        spl_token::state::Account::unpack_from_slice(*premium_temp.try_borrow_data()?)?;
    let buyer_receive_info =
        spl_token::state::Account::unpack_from_slice(*buyer_receive.try_borrow_data()?)?;
    let mint_pda_data = MintPDA::unpack_from_slice(*mint_pda.try_borrow_data()?)?;

    let clock = Clock::get()?;
    let time = (clock.slot * SLOT_MS) as i64 + (clock.unix_timestamp * 1000);

    msg!("asserting validity");
    match contract_pda.contract_state {
        ContractState::ASK => (),
        _ => {
            msg!("INVALID CONTRACT STATE");
            return Err(ProgramError::InvalidArgument);
        }
    };
    if time > contract_pda.contract_data.expiry_date {
        msg!("CONTRACT EXPIRED");
        return Err(ProgramError::InvalidArgument);
    }
    if !system_program::check_id(sys_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !spl_token::check_id(token_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if prem_temp_info.mint != contract_pda.contract_data.premium_type {
        msg!("INCORRECT PREMIUM TYPE");
        return Err(ProgramError::InvalidArgument);
    }
    if prem_temp_info.amount != contract_pda.contract_data.premium_qty {
        msg!("INCORRECT PREMIUM TEMP BALANCE");
        return Err(ProgramError::InvalidArgument);
    }
    let rec_type = match contract_pda.contract_type {
        CALL => contract_pda.contract_data.token_type,
        PUT => contract_pda.contract_data.strike_type,
    };
    if buyer_receive_info.mint != rec_type {
        msg!("INCORRECT BUYER RECEIVE ACCOUNT TYPE");
        return Err(ProgramError::InvalidArgument);
    }
    if !buyer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let s1 = match contract_pda.contract_type {
        CALL => &[0],
        PUT => &[1],
    };
    let mint_seed = get_seed(&contract_pda.contract_data.serialize());
    let (mint_pdak, mint_bump) = Pubkey::find_program_address(&[s1, &mint_seed], program_id);
    if mint_pdak != *mint_pda.key {
        msg!("INCORRECT MINT PDA ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if *holder_mint.key != mint_pda_data.holder_mint {
        msg!("INCORRECT HOLDER MINT ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    let x_ata = get_associated_token_address(buyer.key, holder_mint.key);
    if x_ata != *holder_ata.key {
        msg!("INCORRECT HOLDER MINT ATA");
        return Err(ProgramError::InvalidArgument);
    }

    msg!("transferring premium to writer...");
    let transfer_premium = spl_token::instruction::transfer(
        token_program.key,
        premium_temp.key,
        seller_prem_acc.key,
        buyer.key,
        &[buyer.key],
        contract_pda.contract_data.premium_qty,
    )?;

    invoke(
        &transfer_premium,
        &[premium_temp.clone(), seller_prem_acc.clone(), buyer.clone()],
    )?;

    msg!("minting holder token");
    let ix = spl_token::instruction::mint_to(
        token_program.key,
        holder_mint.key,
        holder_ata.key,
        mint_pda.key,
        &[mint_pda.key],
        1,
    )?;
    invoke_signed(
        &ix,
        &[holder_mint.clone(), holder_ata.clone(), mint_pda.clone()],
        &[&[s1, &mint_seed, &[mint_bump]]],
    )?;

    msg!("updating PDA data...");
    contract_pda.contract_state = ContractState::FINAL;
    contract_pda.buyer_data = Some(PartyData {
        party_pub: buyer.key.clone(),
        temp_pub: premium_temp.key.clone(),
        receive_pub: buyer_receive.key.clone(),
        receive_ata: holder_ata.key.clone(),
    });

    contract_pda.pack_into_slice(*data_pda.data.borrow_mut());

    Ok(())
}

pub fn execute_contract(program_id: &Pubkey, accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    let accounts = &mut accounts.iter();

    let buyer = next_account_info(accounts)?;
    let buyer_temp = next_account_info(accounts)?;
    let buyer_receive = next_account_info(accounts)?;
    let buyer_holder_ata = next_account_info(accounts)?;
    let mint_pda = next_account_info(accounts)?;
    let buyer_holder_mint = next_account_info(accounts)?;
    let writer_temp = next_account_info(accounts)?;
    let data_pda = next_account_info(accounts)?;
    let writer = next_account_info(accounts)?;
    let writer_receive = next_account_info(accounts)?;
    let sys_program = next_account_info(accounts)?;
    let token_program = next_account_info(accounts)?;
    let clock = Clock::get()?;
    let time = (clock.slot * SLOT_MS) as i64 + (clock.unix_timestamp * 1000);

    let contract_pda = ContractPDA::unpack_from_slice(*data_pda.data.borrow())?;
    let ct = contract_pda.contract_type;

    let wd = contract_pda.writer_data.unwrap();

    let mint_pda_data = MintPDA::unpack_from_slice(*mint_pda.try_borrow_data()?)?;
    let buyer_ata_info =
        spl_token::state::Account::unpack_from_slice(*buyer_holder_ata.try_borrow_data()?)?;

    let s1 = match contract_pda.contract_type {
        CALL => &[0],
        PUT => &[1],
    };
    let mint_seed = get_seed(&contract_pda.contract_data.serialize());
    let (mint_pda_k, _mint_bump) = Pubkey::find_program_address(&[s1, &mint_seed], program_id);

    let buyer_temp_info =
        spl_token::state::Account::unpack_from_slice(*buyer_temp.try_borrow_data()?)?;
    let buyer_receive_info =
        spl_token::state::Account::unpack_from_slice(*buyer_receive.try_borrow_data()?)?;

    msg!("asserting validity");
    if time > contract_pda.contract_data.expiry_date {
        msg!("CONTRACT EXPIRED");
        return Err(ProgramError::InvalidArgument);
    }
    if !system_program::check_id(sys_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !spl_token::check_id(token_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    match contract_pda.contract_state {
        ContractState::FINAL => (),
        _ => {
            msg!("CONTRACT NOT FINALISED");
            return Err(ProgramError::InvalidArgument);
        }
    };
    if !buyer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let (temp_type, temp_qty, rec_type) = match ct {
        CALL => (
            contract_pda.contract_data.strike_type,
            contract_pda.contract_data.strike_qty,
            contract_pda.contract_data.token_type,
        ),
        PUT => (
            contract_pda.contract_data.token_type,
            contract_pda.contract_data.token_qty,
            contract_pda.contract_data.strike_type,
        ),
    };
    if buyer_temp_info.mint != temp_type {
        msg!("WRONG STRIKE TYPE");
        return Err(ProgramError::InvalidArgument);
    }
    if buyer_temp_info.amount < temp_qty {
        msg!("WRONG STRIKE TEMP BALANCE");
        return Err(ProgramError::InvalidArgument);
    }
    if *writer_receive.key != wd.receive_pub {
        msg!("WRONG WRITER RECEIVE ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if *writer_temp.key != wd.temp_pub {
        msg!("WRONG ASSET TEMP ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if buyer_receive_info.mint != rec_type {
        msg!("WRONG BUYER RECEIVE ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if mint_pda_k != *mint_pda.key {
        msg!("INVALID MINT PDA ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if mint_pda_data.holder_mint != *buyer_holder_mint.key {
        msg!("INVALID BUYER HOLDER MINT ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if buyer_ata_info.mint != *buyer_holder_mint.key {
        msg!("INVALID BUYER HOLDER ATA ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if buyer_ata_info.owner != *buyer.key {
        msg!("BUYER HOLDER ATA ACCOUNT NOT OWNED BY BUYER");
        return Err(ProgramError::InvalidArgument);
    }

    msg!("burning holder_mint token...");
    let ix1 = spl_token::instruction::burn(
        token_program.key,
        buyer_holder_ata.key,
        buyer_holder_mint.key,
        buyer.key,
        &[buyer.key],
        1,
    )?;
    invoke(
        &ix1,
        &[
            buyer_holder_ata.clone(),
            buyer_holder_mint.clone(),
            buyer.clone(),
        ],
    )?;

    let is_call = match ct {
        CALL => true,
        PUT => false,
    };

    msg!(
        "transferring {} to writer...",
        if is_call { "strike" } else { "asset" }
    );
    let tx1 = spl_token::instruction::transfer(
        token_program.key,
        buyer_temp.key,
        writer_receive.key,
        buyer.key,
        &[buyer.key],
        contract_pda.contract_data.strike_qty,
    )?;

    invoke(
        &tx1,
        &[
            buyer_temp.clone(),
            writer_receive.clone(),
            buyer.clone(),
            token_program.clone(),
        ],
    )?;

    msg!(
        "transferring {} to buyer...",
        if is_call { "asset" } else { "strike" }
    );
    let tx2 = spl_token::instruction::transfer(
        token_program.key,
        writer_temp.key,
        buyer_receive.key,
        data_pda.key,
        &[data_pda.key],
        contract_pda.contract_data.token_qty,
    )?;

    invoke_signed(
        &tx2,
        &[
            writer_temp.clone(),
            buyer_receive.clone(),
            data_pda.clone(),
            token_program.clone(),
        ],
        &[&[
            &contract_pda.seed,
            &contract_pda.index_seed,
            &[contract_pda.bump],
        ]],
    )?;

    msg!(
        "closing {} account...",
        if is_call { "asset_temp" } else { "strike_temp" }
    );
    let tx3 = spl_token::instruction::close_account(
        token_program.key,
        writer_temp.key,
        writer.key,
        data_pda.key,
        &[data_pda.key],
    )?;

    invoke_signed(
        &tx3,
        &[
            writer_temp.clone(),
            writer.clone(),
            data_pda.clone(),
            token_program.clone(),
        ],
        &[&[
            &contract_pda.seed,
            &contract_pda.index_seed,
            &[contract_pda.bump],
        ]],
    )?;

    let send_to = match contract_pda.init_party {
        instruction::InitParty::BUYER => buyer,
        instruction::InitParty::WRITER => writer,
    };

    msg!("zeroing PDA account data...");
    *data_pda.data.borrow_mut() = &mut [];
    msg!("transferring rent from PDA to initialiser...");
    **send_to.try_borrow_mut_lamports()? += data_pda.try_lamports()?;
    **data_pda.try_borrow_mut_lamports()? = 0;
    msg!("PDA account closed");

    Ok(())
}

pub fn expire_contract(accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    let accounts = &mut accounts.iter();
    let writer = next_account_info(accounts)?;
    let writer_temp = next_account_info(accounts)?;
    let data_pda = next_account_info(accounts)?;
    let buyer = next_account_info(accounts)?;
    let sys_program = next_account_info(accounts)?;
    let token_program = next_account_info(accounts)?;

    let contract_pda = ContractPDA::unpack_from_slice(*data_pda.data.borrow())?;
    let clock = Clock::get()?;
    let time = (clock.slot * SLOT_MS) as i64 + (clock.unix_timestamp * 1000);

    msg!("asserting validity...");
    if time < contract_pda.contract_data.expiry_date {
        msg!(
            "timestamp: {}    expiry: {}",
            time,
            contract_pda.contract_data.expiry_date
        );
        msg!("CONTRACT NOT EXPIRED");
        return Err(ProgramError::InvalidArgument);
    }
    if !system_program::check_id(sys_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !spl_token::check_id(token_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    match contract_pda.contract_state {
        ContractState::FINAL => (),
        _ => {
            msg!("CONTRACT NOT FINALISED");
            return Err(ProgramError::InvalidArgument);
        }
    };
    let wd = contract_pda.writer_data.unwrap();
    let bd = contract_pda.buyer_data.unwrap();

    if *writer.key != wd.party_pub {
        msg!("INCORRECT WRITER ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    };
    if *writer_temp.key != wd.temp_pub {
        msg!("INCORRECT ASSET TEMP ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if *buyer.key != bd.party_pub {
        msg!("INCORRECT BUYER ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }

    msg!("transferring writer_temp back to writer...");
    let ix = spl_token::instruction::set_authority(
        token_program.key,
        writer_temp.key,
        Some(writer.key),
        spl_token::instruction::AuthorityType::AccountOwner,
        &data_pda.key,
        &[&data_pda.key],
    )?;
    invoke_signed(
        &ix,
        &[
            writer_temp.clone(),
            writer.clone(),
            data_pda.clone(),
            token_program.clone(),
        ],
        &[&[
            &contract_pda.seed,
            &contract_pda.index_seed,
            &[contract_pda.bump],
        ]],
    )?;

    let send_to = match contract_pda.init_party {
        instruction::InitParty::BUYER => buyer,
        instruction::InitParty::WRITER => writer,
    };

    msg!("zeroing PDA account data...");
    *data_pda.data.borrow_mut() = &mut [];
    msg!("transferring rent from PDA to initialiser...");
    **send_to.try_borrow_mut_lamports()? += data_pda.try_lamports()?;
    **data_pda.try_borrow_mut_lamports()? = 0;
    msg!("PDA account closed");
    Ok(())
}

pub fn cancel_offer(accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    let accounts = &mut accounts.iter();
    let initialiser = next_account_info(accounts)?;
    let token_temp = next_account_info(accounts)?;
    let data_pda = next_account_info(accounts)?;
    let sys_program = next_account_info(accounts)?;
    let token_program = next_account_info(accounts)?;

    let contract_pda = ContractPDA::unpack_from_slice(*data_pda.data.borrow())?;

    msg!("asserting validity...");
    if !system_program::check_id(sys_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !spl_token::check_id(token_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !initialiser.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let pd = match contract_pda.contract_state {
        ContractState::ASK => contract_pda.writer_data.unwrap(),
        ContractState::BID => contract_pda.buyer_data.unwrap(),
        _ => {
            msg!("INVALID CONTRACT STATE");
            return Err(ProgramError::InvalidArgument);
        }
    };
    if *initialiser.key != pd.party_pub {
        msg!("INCORRECT INITIALISER ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if *token_temp.key != pd.temp_pub {
        msg!("INCORRECT ASSET_TEMP ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }

    msg!("transferring temp back to initialiser...");
    let ix = spl_token::instruction::set_authority(
        token_program.key,
        token_temp.key,
        Some(initialiser.key),
        spl_token::instruction::AuthorityType::AccountOwner,
        &data_pda.key,
        &[&data_pda.key],
    )?;
    invoke_signed(
        &ix,
        &[
            token_temp.clone(),
            initialiser.clone(),
            data_pda.clone(),
            token_program.clone(),
        ],
        &[&[
            &contract_pda.seed,
            &contract_pda.index_seed,
            &[contract_pda.bump],
        ]],
    )?;

    msg!("zeroing PDA account data...");
    *data_pda.data.borrow_mut() = &mut [];
    msg!("transferring rent from PDA to initialiser...");
    **initialiser.try_borrow_mut_lamports()? += data_pda.try_lamports()?;
    **data_pda.try_borrow_mut_lamports()? = 0;
    msg!("PDA account closed");
    Ok(())
}

pub fn create_mint(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    seeds: (u8, [u8; 32]),
) -> Result<(), ProgramError> {
    let accounts = &mut accounts.iter();

    let sender = next_account_info(accounts)?;
    let holder_mint = next_account_info(accounts)?;
    let mint_pda = next_account_info(accounts)?;
    let sys_program = next_account_info(accounts)?;
    let token_program = next_account_info(accounts)?;
    let rent_program = next_account_info(accounts)?;

    msg!("asserting validity...");
    if !system_program::check_id(sys_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !spl_token::check_id(token_program.key) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !sender.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !holder_mint.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let pda_data = MintPDA {
        holder_mint: holder_mint.key.clone(),
    };

    let (_pda, bump) = Pubkey::find_program_address(&[&[seeds.0], &seeds.1], program_id);
    let min_rent = rent::Rent::get()?.minimum_balance(MintPDA::LEN);

    msg!("creating mint PDA");
    let ix = system_instruction::create_account(
        sender.key,
        mint_pda.key,
        min_rent,
        MintPDA::LEN as u64,
        program_id,
    );
    invoke_signed(
        &ix,
        &[sender.clone(), mint_pda.clone(), sys_program.clone()],
        &[&[&[seeds.0], &seeds.1, &[bump]]],
    )?;

    pda_data.pack_into_slice(*mint_pda.try_borrow_mut_data()?);

    msg!("creating mint account");
    let min_rent = rent::Rent::get()?.minimum_balance(82);
    let ix = system_instruction::create_account(
        sender.key,
        holder_mint.key,
        min_rent,
        82,
        &spl_token::id(),
    );
    invoke(&ix, &[sender.clone(), holder_mint.clone()])?;

    msg!("initialising mint account");
    let ix = spl_token::instruction::initialize_mint(
        &spl_token::id(),
        holder_mint.key,
        mint_pda.key,
        Some(mint_pda.key),
        0,
    )?;
    invoke(&ix, &[holder_mint.clone(), rent_program.clone()])?;
    Ok(())
}
