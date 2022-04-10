use crate::instruction;
use crate::state::{ContractPDA, ContractState, ContractType::*, PartyData};
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
    } = instruction;

    let min_rent = rent::Rent::get()?.minimum_balance(ContractPDA::LEN);

    let accounts = &mut accounts.iter();

    let initialiser = next_account_info(accounts)?;
    let token_temp = next_account_info(accounts)?;
    let receive_acc = next_account_info(accounts)?;
    let prem_receive = match party {
        instruction::InitParty::WRITER => Some(next_account_info(accounts)?),
        _ => None,
    };
    let data_pda = next_account_info(accounts)?;
    let sys_program = next_account_info(accounts)?;
    let token_program = next_account_info(accounts)?;

    let token_temp_info =
        spl_token::state::Account::unpack_from_slice(*token_temp.try_borrow_data()?)?;
    let rec_account_info =
        spl_token::state::Account::unpack_from_slice(*receive_acc.try_borrow_data()?)?;

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

    if let Some(acc) = prem_receive {
        let acc_info = spl_token::state::Account::unpack_from_slice(*acc.try_borrow_data()?)?;
        if acc_info.mint != contract_data.premium_type {
            msg!("INCORRECT PREMIUM RECEIVE ACCOUNT TYPE");
            return Err(ProgramError::InvalidInstructionData);
        };
    }

    msg!("building PDA data...");
    let pda_data = match party {
        instruction::InitParty::BUYER => ContractPDA {
            contract_data,
            contract_state: ContractState::BID,
            buyer_data: Some(PartyData {
                party_pub: initialiser.key.clone(),
                temp_pub: token_temp.key.clone(),
                receive_pub: receive_acc.key.clone(),
                prem_receive_pub: None,
            }),
            writer_data: None,
            is_initialised: true,
            bump,
            seed,
            init_party: instruction::InitParty::BUYER,
            contract_type,
        },
        instruction::InitParty::WRITER => ContractPDA {
            contract_data,
            contract_state: ContractState::ASK,
            buyer_data: None,
            writer_data: Some(PartyData {
                party_pub: initialiser.key.clone(),
                temp_pub: token_temp.key.clone(),
                receive_pub: receive_acc.key.clone(),
                prem_receive_pub: Some(prem_receive.unwrap().key.clone()),
            }),
            is_initialised: true,
            bump,
            seed,
            init_party: instruction::InitParty::WRITER,
            contract_type,
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
        &[&[&seed, &[bump]]],
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

pub fn accept_bid(accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    let accounts = &mut accounts.iter();

    let writer = next_account_info(accounts)?;
    let writer_temp = next_account_info(accounts)?;
    let writer_receive = next_account_info(accounts)?;
    let prem_receive = next_account_info(accounts)?;
    let data_pda = next_account_info(accounts)?;
    let premium_temp = next_account_info(accounts)?;
    let buyer = next_account_info(accounts)?;
    let sys_program = next_account_info(accounts)?;
    let token_program = next_account_info(accounts)?;

    if data_pda.try_data_is_empty()? {
        return Err(ProgramError::InvalidAccountData);
    }

    let writer_temp_info =
        spl_token::state::Account::unpack_from_slice(*writer_temp.try_borrow_data()?)?;
    let writer_receive_info =
        spl_token::state::Account::unpack_from_slice(*writer_receive.try_borrow_data()?)?;
    let prem_receive_info =
        spl_token::state::Account::unpack_from_slice(*writer_receive.try_borrow_data()?)?;
    let premium_temp_info =
        spl_token::state::Account::unpack_from_slice(*premium_temp.try_borrow_data()?)?;
    let mut contract_pda = ContractPDA::unpack_from_slice(*data_pda.try_borrow_data()?)?;

    let clock = Clock::get()?;
    let time = (clock.slot * SLOT_MS) as i64 + (clock.unix_timestamp * 1000);

    let bd = contract_pda.buyer_data.unwrap();

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
    if prem_receive_info.mint != contract_pda.contract_data.premium_type {
        msg!("INCORRECT PREMIUM_RECEIVE ACCOUNT TOKEN TYPE");
        return Err(ProgramError::InvalidArgument);
    }
    if *buyer.key != bd.party_pub {
        msg!("INCORRECT BUYER ACCOUNT");
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
        prem_receive.key,
        data_pda.key,
        &[data_pda.key],
        premium_temp_info.amount,
    )?;

    invoke_signed(
        &transfer_prem,
        &[
            premium_temp.clone(),
            prem_receive.clone(),
            data_pda.clone(),
            token_program.clone(),
        ],
        &[&[&contract_pda.seed, &[contract_pda.bump]]],
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
        &[&[&contract_pda.seed, &[contract_pda.bump]]],
    )?;

    msg!("updating PDA data...");
    contract_pda.contract_state = ContractState::FINAL;
    contract_pda.writer_data = Some(PartyData {
        party_pub: writer.key.clone(),
        temp_pub: writer_temp.key.clone(),
        receive_pub: writer_receive.key.clone(),
        prem_receive_pub: Some(prem_receive.key.clone()),
    });
    contract_pda.buyer_data = Some(bd);

    contract_pda.pack_into_slice(*data_pda.data.borrow_mut());
    Ok(())
}

pub fn accept_ask(accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    let accounts = &mut accounts.iter();
    let buyer = next_account_info(accounts)?;
    let premium_temp = next_account_info(accounts)?;
    let buyer_receive = next_account_info(accounts)?;
    let data_pda = next_account_info(accounts)?;
    let seller_prem_acc = next_account_info(accounts)?;
    let sys_program = next_account_info(accounts)?;
    let token_program = next_account_info(accounts)?;

    let mut contract_pda = ContractPDA::unpack_from_slice(*data_pda.try_borrow_data()?)?;

    let prem_temp_info =
        spl_token::state::Account::unpack_from_slice(*premium_temp.try_borrow_data()?)?;
    let buyer_receive_info =
        spl_token::state::Account::unpack_from_slice(*buyer_receive.try_borrow_data()?)?;

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
    msg!("updating PDA data...");
    contract_pda.contract_state = ContractState::FINAL;
    contract_pda.buyer_data = Some(PartyData {
        party_pub: buyer.key.clone(),
        temp_pub: premium_temp.key.clone(),
        receive_pub: buyer_receive.key.clone(),
        prem_receive_pub: None,
    });

    contract_pda.pack_into_slice(*data_pda.data.borrow_mut());

    Ok(())
}

pub fn execute_contract(accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    let accounts = &mut accounts.iter();

    let buyer = next_account_info(accounts)?;
    let buyer_temp = next_account_info(accounts)?;
    let buyer_receive = next_account_info(accounts)?;
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
    let bd = contract_pda.buyer_data.unwrap();

    let buyer_temp_info =
        spl_token::state::Account::unpack_from_slice(*buyer_temp.try_borrow_data()?)?;

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
    if bd.party_pub != *buyer.key {
        msg!("WRONG BUYER ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }
    if !buyer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let (temp_type, temp_qty) = match ct {
        CALL => (
            contract_pda.contract_data.strike_type,
            contract_pda.contract_data.strike_qty,
        ),
        PUT => (
            contract_pda.contract_data.token_type,
            contract_pda.contract_data.token_qty,
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
    if *buyer_receive.key != bd.receive_pub {
        msg!("WRONG BUYER RECEIVE ACCOUNT");
        return Err(ProgramError::InvalidArgument);
    }

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
        &[&[&contract_pda.seed, &[contract_pda.bump]]],
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
        &[&[&contract_pda.seed, &[contract_pda.bump]]],
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
        &[&[&contract_pda.seed, &[contract_pda.bump]]],
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
        &[&[&contract_pda.seed, &[contract_pda.bump]]],
    )?;

    msg!("zeroing PDA account data...");
    *data_pda.data.borrow_mut() = &mut [];
    msg!("transferring rent from PDA to initialiser...");
    **initialiser.try_borrow_mut_lamports()? += data_pda.try_lamports()?;
    **data_pda.try_borrow_mut_lamports()? = 0;
    msg!("PDA account closed");
    Ok(())
}
