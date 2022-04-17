#![cfg(feature = "test-bpf")]

use sol_options::{
    entrypoint::process_instruction,
    instruction::InitParty,
    state::{get_seed, ContractData, ContractPDA, ContractState, ContractType, PartyData},
};
use solana_program_test::*;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    signer::{keypair, Signer},
    system_instruction, system_program, sysvar,
    transaction::Transaction,
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token;
use std::time::SystemTime;

const MINT_SIZE: u64 = 82;

struct PartyKeys {
    main: keypair::Keypair,
    mint_1: Pubkey,
    mint_2: Pubkey,
}

struct TestEnv {
    ctx: ProgramTestContext,
    program_key: keypair::Keypair,
    buyer: PartyKeys,
    writer: PartyKeys,
    mint_1: keypair::Keypair,
    mint_2: keypair::Keypair,
    holder_mint: keypair::Keypair,
    buyer_temp: Pubkey,
    writer_temp: Pubkey,
    buyer_holder: Pubkey,
    index_seed: [u8; 41],
}

enum InitMode {
    BUYER,
    WRITER,
}

#[tokio::test]
async fn call_bid_execute() {
    let contract_type = ContractType::CALL;
    let init_mode = InitMode::BUYER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 10000;
    let (contract_pda, mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    let contract_pda = accept_bid(&mut test_env, contract_pda, mint_pda).await;
    execute(&mut test_env, contract_pda, mint_pda, &contract_type).await;
}

#[tokio::test]
async fn call_ask_execute() {
    let contract_type = ContractType::CALL;
    let init_mode = InitMode::WRITER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 10000;
    let (contract_pda, mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    let contract_pda = accept_ask(&mut test_env, contract_pda, mint_pda).await;
    execute(&mut test_env, contract_pda, mint_pda, &contract_type).await;
}

#[tokio::test]
async fn call_bid_cancel() {
    let contract_type = ContractType::CALL;
    let init_mode = InitMode::BUYER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 10000;
    let (contract_pda, _mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    cancel_offer(&mut test_env, contract_pda, &init_mode).await;
}

#[tokio::test]
async fn call_ask_cancel() {
    let contract_type = ContractType::CALL;
    let init_mode = InitMode::WRITER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 10000;
    let (contract_pda, _mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    cancel_offer(&mut test_env, contract_pda, &init_mode).await;
}

#[tokio::test]
async fn call_bid_expire() {
    let contract_type = ContractType::CALL;
    let init_mode = InitMode::BUYER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 1000;
    let (contract_pda, mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    let contract_pda = accept_bid(&mut test_env, contract_pda, mint_pda).await;
    test_env.ctx.warp_to_slot(10).unwrap();
    expire_contract(&mut test_env, contract_pda).await;
}

#[tokio::test]
async fn call_ask_expire() {
    let contract_type = ContractType::CALL;
    let init_mode = InitMode::WRITER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 1000;
    let (contract_pda, mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    let contract_pda = accept_ask(&mut test_env, contract_pda, mint_pda).await;
    test_env.ctx.warp_to_slot(10).unwrap();
    expire_contract(&mut test_env, contract_pda).await;
}

#[tokio::test]
async fn put_bid_execute() {
    let contract_type = ContractType::PUT;
    let init_mode = InitMode::BUYER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 10000;
    let (contract_pda, mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    let contract_pda = accept_bid(&mut test_env, contract_pda, mint_pda).await;
    execute(&mut test_env, contract_pda, mint_pda, &contract_type).await;
}

#[tokio::test]
async fn put_ask_execute() {
    let contract_type = ContractType::PUT;
    let init_mode = InitMode::WRITER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 10000;
    let (contract_pda, mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    let contract_pda = accept_ask(&mut test_env, contract_pda, mint_pda).await;
    execute(&mut test_env, contract_pda, mint_pda, &contract_type).await;
}

#[tokio::test]
async fn put_bid_cancel() {
    let contract_type = ContractType::PUT;
    let init_mode = InitMode::BUYER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 10000;
    let (contract_pda, _mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    cancel_offer(&mut test_env, contract_pda, &init_mode).await;
}

#[tokio::test]
async fn put_ask_cancel() {
    let contract_type = ContractType::PUT;
    let init_mode = InitMode::WRITER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 10000;
    let (contract_pda, _mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    cancel_offer(&mut test_env, contract_pda, &init_mode).await;
}

#[tokio::test]
async fn put_bid_expire() {
    let contract_type = ContractType::PUT;
    let init_mode = InitMode::BUYER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 1000;
    let (contract_pda, mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    let contract_pda = accept_bid(&mut test_env, contract_pda, mint_pda).await;
    test_env.ctx.warp_to_slot(10).unwrap();
    expire_contract(&mut test_env, contract_pda).await;
}

#[tokio::test]
async fn put_ask_expire() {
    let contract_type = ContractType::PUT;
    let init_mode = InitMode::WRITER;
    let mut test_env = init_env(contract_type, &init_mode).await;
    let expire_time = 1000;
    let (contract_pda, mint_pda) =
        init_contract(&mut test_env, &init_mode, &contract_type, expire_time).await;
    let contract_pda = accept_ask(&mut test_env, contract_pda, mint_pda).await;
    test_env.ctx.warp_to_slot(10).unwrap();
    expire_contract(&mut test_env, contract_pda).await;
}

async fn init_env(contract_type: ContractType, init_mode: &InitMode) -> TestEnv {
    println!("\n-----CREATING-TEST-ENVIRONMENT-----\n");
    let program_key = keypair::Keypair::new();
    let buyer_key = keypair::Keypair::new();
    let writer_key = keypair::Keypair::new();
    let mint_1 = keypair::Keypair::new();
    let mint_2 = keypair::Keypair::new();
    let buyer_temp = keypair::Keypair::new();
    let writer_temp = keypair::Keypair::new();
    let holder_mint = keypair::Keypair::new();

    println!("starting test-server");

    let mut ctx = ProgramTest::new(
        "call_opt",
        program_key.pubkey(),
        processor!(process_instruction),
    )
    .start_with_context()
    .await;

    let client = &mut ctx.banks_client;
    let payer = &ctx.payer;
    let block = ctx.last_blockhash.clone();

    println!("started test-server, creating mint account instructions...");
    let min_rent = Rent::default().minimum_balance(MINT_SIZE as usize);

    let c1 = system_instruction::create_account(
        &payer.pubkey(),
        &mint_1.pubkey(),
        min_rent,
        MINT_SIZE,
        &spl_token::id(),
    );
    let c2 = system_instruction::create_account(
        &payer.pubkey(),
        &mint_2.pubkey(),
        min_rent,
        MINT_SIZE,
        &spl_token::id(),
    );

    let i1 = spl_token::instruction::initialize_mint(
        &spl_token::id(),
        &mint_1.pubkey(),
        &payer.pubkey(),
        None,
        1,
    )
    .expect("could not create initialise_mint instruction");

    let i2 = spl_token::instruction::initialize_mint(
        &spl_token::id(),
        &mint_2.pubkey(),
        &payer.pubkey(),
        None,
        1,
    )
    .expect("could not create initialise_mint instruction");

    println!("creating mint 1...");
    let tx = Transaction::new_signed_with_payer(
        &[c1, i1],
        Some(&payer.pubkey()),
        &[payer, &mint_1],
        block,
    );
    client.process_transaction(tx).await.unwrap();

    println!("creating mint 2...");
    let tx = Transaction::new_signed_with_payer(
        &[c2, i2],
        Some(&payer.pubkey()),
        &[payer, &mint_2],
        block,
    );
    client.process_transaction(tx).await.unwrap();

    println!("transferring funds from payer to parties...");
    let split_txs = system_instruction::transfer_many(
        &payer.pubkey(),
        &[
            (buyer_key.pubkey(), 100000000),
            (writer_key.pubkey(), 100000000),
        ],
    );
    let tx = Transaction::new_signed_with_payer(
        &split_txs[0..2],
        Some(&payer.pubkey()),
        &[payer],
        block,
    );
    client.process_transaction(tx).await.unwrap();

    println!("creating ATA accounts...");
    let b1 =
        create_associated_token_account(&payer.pubkey(), &buyer_key.pubkey(), &mint_1.pubkey());
    let b2 =
        create_associated_token_account(&payer.pubkey(), &buyer_key.pubkey(), &mint_2.pubkey());
    let b3 =
        create_associated_token_account(&payer.pubkey(), &writer_key.pubkey(), &mint_1.pubkey());
    let b4 =
        create_associated_token_account(&payer.pubkey(), &writer_key.pubkey(), &mint_2.pubkey());

    let tx = Transaction::new_signed_with_payer(
        &[b1, b2, b3, b4],
        Some(&payer.pubkey()),
        &[payer],
        block,
    );
    client.process_transaction(tx).await.unwrap();

    let b1 = get_associated_token_address(&buyer_key.pubkey(), &mint_1.pubkey());
    let b2 = get_associated_token_address(&buyer_key.pubkey(), &mint_2.pubkey());
    let b3 = get_associated_token_address(&buyer_key.pubkey(), &holder_mint.pubkey());
    let w1 = get_associated_token_address(&writer_key.pubkey(), &mint_1.pubkey());
    let w2 = get_associated_token_address(&writer_key.pubkey(), &mint_2.pubkey());

    let min_rent = Rent::default().minimum_balance(165);

    println!("creating buyer and writer temps");
    let bt1 = system_instruction::create_account(
        &buyer_key.pubkey(),
        &buyer_temp.pubkey(),
        min_rent,
        165,
        &spl_token::id(),
    );
    let wt1 = system_instruction::create_account(
        &writer_key.pubkey(),
        &writer_temp.pubkey(),
        min_rent,
        165,
        &spl_token::id(),
    );

    println!("initialising buyer and writer temps");
    let ibt1 = spl_token::instruction::initialize_account(
        &spl_token::id(),
        &buyer_temp.pubkey(),
        &mint_2.pubkey(),
        &buyer_key.pubkey(),
    )
    .unwrap();
    let iwt1 = spl_token::instruction::initialize_account(
        &spl_token::id(),
        &writer_temp.pubkey(),
        &mint_1.pubkey(),
        &writer_key.pubkey(),
    )
    .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[bt1, ibt1],
        Some(&payer.pubkey()),
        &[&buyer_temp, &buyer_key, &payer],
        block,
    );
    client.process_transaction(tx).await.unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[wt1, iwt1],
        Some(&payer.pubkey()),
        &[payer, &writer_key, &writer_temp],
        block,
    );

    client.process_transaction(tx).await.unwrap();

    println!("minting premium, strike and assets");
    let prem_mint = spl_token::instruction::mint_to(
        &spl_token::id(),
        &mint_2.pubkey(),
        &buyer_temp.pubkey(),
        &payer.pubkey(),
        &[&payer.pubkey()],
        5,
    )
    .unwrap();

    let wtp = writer_temp.pubkey();
    let (strike_type, strike_acc) = match contract_type {
        ContractType::CALL => (mint_2.pubkey(), &b2),
        ContractType::PUT => (mint_1.pubkey(), &wtp),
    };
    let strike_mint = spl_token::instruction::mint_to(
        &spl_token::id(),
        &strike_type,
        strike_acc,
        &payer.pubkey(),
        &[&payer.pubkey()],
        5,
    )
    .unwrap();

    let (asset_type, asset_acc) = match contract_type {
        ContractType::CALL => (mint_1.pubkey(), &wtp),
        ContractType::PUT => (mint_2.pubkey(), &b2),
    };
    let asset_mint = spl_token::instruction::mint_to(
        &spl_token::id(),
        &asset_type,
        asset_acc,
        &payer.pubkey(),
        &[&payer.pubkey()],
        5,
    )
    .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[prem_mint, strike_mint, asset_mint],
        Some(&payer.pubkey()),
        &[payer],
        block,
    );
    client.process_transaction(tx).await.unwrap();

    let buyer = PartyKeys {
        main: buyer_key,
        mint_1: b1,
        mint_2: b2,
    };

    let writer = PartyKeys {
        main: writer_key,
        mint_1: w1,
        mint_2: w2,
    };

    let mut index_seed = [0; 41];

    match init_mode {
        InitMode::BUYER => index_seed[..32].copy_from_slice(&buyer.main.pubkey().to_bytes()),
        InitMode::WRITER => index_seed[..32].copy_from_slice(&writer.main.pubkey().to_bytes()),
    };
    index_seed[32] = match contract_type {
        ContractType::CALL => 0,
        ContractType::PUT => 1,
    };

    println!("\n\n-----TEST-ENVIRONMENT-SETUP-COMPLETE-----\n\n");

    TestEnv {
        ctx,
        buyer,
        writer,
        program_key,
        mint_1,
        mint_2,
        holder_mint,
        buyer_temp: buyer_temp.pubkey(),
        buyer_holder: b3,
        writer_temp: writer_temp.pubkey(),
        index_seed,
    }
}

async fn init_contract(
    test_env: &mut TestEnv,
    init_mode: &InitMode,
    contract_type: &ContractType,
    expire_time: i64,
) -> (ContractPDA, Pubkey) {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    println!("creating contract + instruction data");
    let (token_type, strike_type, premium_type, s1) = match contract_type {
        ContractType::CALL => (
            test_env.mint_1.pubkey(),
            test_env.mint_2.pubkey(),
            test_env.mint_2.pubkey(),
            [0],
        ),
        ContractType::PUT => (
            test_env.mint_2.pubkey(),
            test_env.mint_1.pubkey(),
            test_env.mint_2.pubkey(),
            [1],
        ),
    };

    let contract_data = ContractData {
        token_type,
        token_qty: 5,
        strike_type,
        strike_qty: 5,
        premium_type,
        premium_qty: 5,
        expiry_date: now + expire_time,
    };

    let mint_seed = get_seed(&contract_data.serialize());
    println!("creating holder mint account");
    let (mint_pda, _mint_bump) =
        Pubkey::find_program_address(&[&s1, &mint_seed], &test_env.program_key.pubkey());

    // let min_rent = Rent::default().minimum_balance(82);
    // let ix1 = system_instruction::create_account(
    //     &test_env.ctx.payer.pubkey(),
    //     &test_env.holder_mint.pubkey(),
    //     min_rent,
    //     82,
    //     &spl_token::id(),
    // );

    let accs = vec![
        AccountMeta {
            pubkey: test_env.ctx.payer.pubkey(),
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.holder_mint.pubkey(),
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: mint_pda,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: system_program::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: spl_token::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: sysvar::rent::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    let mut ixd = [0; 34];
    ixd[0] = 7;
    ixd[1] = s1[0];
    ixd[2..].copy_from_slice(&mint_seed);

    let ix = Instruction::new_with_bytes(test_env.program_key.pubkey(), &ixd, accs);
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, &test_env.holder_mint],
        test_env.ctx.last_blockhash,
    );
    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    println!("creating holder_ata");
    let ix = create_associated_token_account(
        &test_env.ctx.payer.pubkey(),
        &test_env.buyer.main.pubkey(),
        &test_env.holder_mint.pubkey(),
    );
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer],
        test_env.ctx.last_blockhash,
    );
    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    let buyer_data = match init_mode {
        InitMode::BUYER => Some(PartyData {
            party_pub: test_env.buyer.main.pubkey(),
            temp_pub: test_env.buyer_temp.clone(),
            receive_pub: test_env.buyer.mint_1.clone(),
            receive_ata: test_env.buyer_holder.clone(),
        }),
        InitMode::WRITER => None,
    };

    let writer_data = match init_mode {
        InitMode::BUYER => None,
        InitMode::WRITER => Some(PartyData {
            party_pub: test_env.writer.main.pubkey(),
            temp_pub: test_env.writer_temp.clone(),
            receive_pub: test_env.writer.mint_2.clone(),
            receive_ata: test_env.writer.mint_2.clone(),
        }),
    };

    let mut instruction_data = [0; 171];
    instruction_data[0] = match init_mode {
        InitMode::BUYER => 0,
        InitMode::WRITER => 1,
    };
    instruction_data[1] = match contract_type {
        ContractType::CALL => 0,
        ContractType::PUT => 1,
    };
    let cd = contract_data.serialize();
    instruction_data[2..ContractData::LEN + 2].copy_from_slice(&cd);
    instruction_data[ContractData::LEN + 2..].copy_from_slice(&test_env.index_seed);
    let seed = get_seed(&cd);
    let index_seed = get_seed(&test_env.index_seed);

    let (pda, bump) =
        Pubkey::find_program_address(&[&seed, &index_seed], &test_env.program_key.pubkey());

    let (initialiser, token_temp, receive_acc, receive_ata) = match init_mode {
        InitMode::BUYER => (
            test_env.buyer.main.pubkey(),
            test_env.buyer_temp.clone(),
            test_env.buyer.mint_1.clone(),
            test_env.buyer_holder.clone(),
        ),
        InitMode::WRITER => (
            test_env.writer.main.pubkey(),
            test_env.writer_temp.clone(),
            test_env.writer.mint_2.clone(),
            test_env.writer.mint_2.clone(),
        ),
    };

    let accounts = vec![
        AccountMeta {
            pubkey: initialiser,
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: token_temp,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: receive_acc,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: receive_ata,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: mint_pda.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.holder_mint.pubkey(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: pda,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: system_program::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: spl_token::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    println!("sending initialise contract instruction...");
    let instruction =
        Instruction::new_with_bytes(test_env.program_key.pubkey(), &instruction_data, accounts);

    let signer = match init_mode {
        InitMode::BUYER => &test_env.buyer.main,
        InitMode::WRITER => &test_env.writer.main,
    };
    let tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, signer],
        test_env.ctx.last_blockhash.clone(),
    );

    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    println!("asserting contract state");
    let pda_account = test_env
        .ctx
        .banks_client
        .get_account(pda)
        .await
        .unwrap()
        .expect("could not find PDA account");

    let pda_data = ContractPDA::unpack_from_slice(&pda_account.data[..]).unwrap();

    let contract_state = match init_mode {
        InitMode::BUYER => ContractState::BID,
        InitMode::WRITER => ContractState::ASK,
    };
    let init_party = match init_mode {
        InitMode::BUYER => InitParty::BUYER,
        InitMode::WRITER => InitParty::WRITER,
    };

    let expected_data = ContractPDA {
        contract_data,
        contract_state,
        buyer_data,
        writer_data,
        is_initialised: true,
        seed,
        bump,
        init_party,
        contract_type: *contract_type,
        index_seed: get_seed(&test_env.index_seed),
    };

    assert_eq!(expected_data, pda_data, "incorrect PDA data");

    println!("trying illegal transaction...");
    let (temp, dest, kp) = match init_mode {
        InitMode::BUYER => (
            &test_env.buyer_temp,
            &test_env.buyer.mint_2,
            &test_env.buyer.main,
        ),
        InitMode::WRITER => (
            &test_env.writer_temp,
            &test_env.writer.mint_1,
            &test_env.writer.main,
        ),
    };
    let ix = spl_token::instruction::transfer(
        &spl_token::id(),
        temp,
        dest,
        &kp.pubkey(),
        &[&kp.pubkey()],
        5,
    )
    .unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, kp],
        test_env.ctx.last_blockhash,
    );
    let tx_result = test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .expect_err("illegal transaction did not fail");
    println!("illegal transaction failed: {:?}", tx_result);
    (pda_data, mint_pda)
}

async fn accept_bid(
    test_env: &mut TestEnv,
    contract_pda: ContractPDA,
    mint_pda: Pubkey,
) -> ContractPDA {
    let (pda, _bump) = Pubkey::find_program_address(
        &[&contract_pda.seed, &contract_pda.index_seed],
        &test_env.program_key.pubkey(),
    );

    let prem_init_balance = get_token_balance(test_env, test_env.writer.mint_2.clone()).await;

    println!("creating accept-bid instruction...");

    let accounts = vec![
        AccountMeta {
            pubkey: test_env.writer.main.pubkey(),
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.writer_temp.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.writer.mint_2.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.writer.mint_2.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: pda,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.buyer_temp.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.buyer.main.pubkey(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.buyer_holder.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: mint_pda,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.holder_mint.pubkey(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: system_program::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: spl_token::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    let instruction_data = &[2];

    let ix = Instruction::new_with_bytes(test_env.program_key.pubkey(), instruction_data, accounts);
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.writer.main.pubkey()),
        &[&test_env.writer.main],
        test_env.ctx.last_blockhash.clone(),
    );
    println!("sending accept-bid instruction...");
    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    println!("asserting contract + party states");
    let pda_account = test_env
        .ctx
        .banks_client
        .get_account(pda)
        .await
        .unwrap()
        .expect("could not find PDA account");

    let pda_data = ContractPDA::unpack_from_slice(&pda_account.data[..]).unwrap();

    let writer_data = Some(PartyData {
        party_pub: test_env.writer.main.pubkey(),
        temp_pub: test_env.writer_temp.clone(),
        receive_pub: test_env.writer.mint_2.clone(),
        receive_ata: test_env.writer.mint_2.clone(),
    });

    let expected_data = ContractPDA {
        writer_data,
        contract_state: ContractState::FINAL,
        ..contract_pda
    };

    assert_eq!(expected_data, pda_data, "incorrect PDA data");

    let prem_balance = get_token_balance(test_env, test_env.writer.mint_2.clone()).await;

    let prem_paid = prem_balance - prem_init_balance;
    assert_eq!(
        prem_paid, expected_data.contract_data.premium_qty,
        "incorrect writer premium balance"
    );

    let buyer_holder_balance = get_token_balance(test_env, test_env.buyer_holder.clone()).await;

    assert_eq!(
        buyer_holder_balance, 1,
        "incorrect buyer holder token balance"
    );

    let ix = spl_token::instruction::transfer(
        &spl_token::id(),
        &test_env.writer_temp,
        &test_env.writer.mint_1,
        &test_env.writer.main.pubkey(),
        &[&test_env.writer.main.pubkey()],
        5,
    )
    .unwrap();

    println!("trying illegal transaction...");
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, &test_env.writer.main],
        test_env.ctx.last_blockhash,
    );
    let tx_err = test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .expect_err("illegal transaction did not fail");
    println!("illegal transaction failed: {:?}", tx_err);

    expected_data
}

async fn accept_ask(
    test_env: &mut TestEnv,
    contract_pda: ContractPDA,
    mint_pda: Pubkey,
) -> ContractPDA {
    let (pda, _bump) = Pubkey::find_program_address(
        &[&contract_pda.seed, &contract_pda.index_seed],
        &test_env.program_key.pubkey(),
    );

    let prem_init_balance = get_token_balance(test_env, test_env.writer.mint_2.clone()).await;

    println!("creating accept-ask instruction");

    let accounts = vec![
        AccountMeta {
            pubkey: test_env.buyer.main.pubkey(),
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.buyer_temp.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.buyer.mint_1.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.buyer_holder.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: mint_pda,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.holder_mint.pubkey(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: pda,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.writer.mint_2.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: system_program::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: spl_token::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    let instruction_data = &[3];

    let ix = Instruction::new_with_bytes(test_env.program_key.pubkey(), instruction_data, accounts);
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, &test_env.buyer.main],
        test_env.ctx.last_blockhash.clone(),
    );
    println!("sending accept-ask transaction...");
    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    println!("asserting contract + party states");
    let buyer_data = Some(PartyData {
        party_pub: test_env.buyer.main.pubkey(),
        temp_pub: test_env.buyer_temp.clone(),
        receive_pub: test_env.buyer.mint_1.clone(),
        receive_ata: test_env.buyer_holder.clone(),
    });

    let expected_data = ContractPDA {
        buyer_data,
        contract_state: ContractState::FINAL,
        ..contract_pda
    };

    let pda_account = test_env
        .ctx
        .banks_client
        .get_account(pda)
        .await
        .unwrap()
        .expect("could not find PDA account");

    let pda_data = ContractPDA::unpack_from_slice(&pda_account.data[..]).unwrap();

    assert_eq!(expected_data, pda_data, "incorrect PDA data");

    let prem_balance = get_token_balance(test_env, test_env.writer.mint_2.clone()).await;

    let prem_paid = prem_balance - prem_init_balance;
    assert_eq!(
        prem_paid, expected_data.contract_data.premium_qty,
        "incorrect writer premium balance"
    );

    let buyer_holder_balance = get_token_balance(test_env, test_env.buyer_holder.clone()).await;
    assert_eq!(
        buyer_holder_balance, 1,
        "incorrect buyer holder token balance"
    );

    println!("trying illegal transaction...");
    let ix = spl_token::instruction::transfer(
        &spl_token::id(),
        &test_env.writer_temp,
        &test_env.writer.mint_1,
        &test_env.writer.main.pubkey(),
        &[&test_env.writer.main.pubkey()],
        5,
    )
    .unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, &test_env.writer.main],
        test_env.ctx.last_blockhash,
    );
    let tx_err = test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .expect_err("illegal transaction did not fail");
    println!("illegal transaction failed: {:?}", tx_err);

    expected_data
}

async fn execute(
    test_env: &mut TestEnv,
    contract_pda: ContractPDA,
    mint_pda: Pubkey,
    contract_type: &ContractType,
) {
    let (pda, _bump) = Pubkey::find_program_address(
        &[&contract_pda.seed, &contract_pda.index_seed],
        &test_env.program_key.pubkey(),
    );

    let (strike_rec_pub, asset_rec_pub) = match contract_type {
        ContractType::CALL => (test_env.writer.mint_2, test_env.buyer.mint_1),
        ContractType::PUT => (test_env.buyer.mint_1, test_env.writer.mint_2),
    };
    let strike_init_balance = get_token_balance(test_env, strike_rec_pub.clone()).await;
    let asset_init_balance = get_token_balance(test_env, asset_rec_pub.clone()).await;

    println!("creating execute transaction");
    let accounts = vec![
        AccountMeta {
            pubkey: test_env.buyer.main.pubkey(),
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.buyer.mint_2.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.buyer.mint_1.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.buyer_holder.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: mint_pda,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.holder_mint.pubkey(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.writer_temp.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: pda,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.writer.main.pubkey(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.writer.mint_2.clone(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: system_program::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: spl_token::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    let instruction_data = &[5];

    let ix = Instruction::new_with_bytes(test_env.program_key.pubkey(), instruction_data, accounts);
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.buyer.main.pubkey()),
        &[&test_env.buyer.main],
        test_env.ctx.last_blockhash,
    );
    println!("sending execute transaction...");
    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();
    println!("asserting contract + party states...");
    test_env
        .ctx
        .banks_client
        .get_account(pda)
        .await
        .unwrap()
        .ok_or("")
        .expect_err("PDA account not closed");

    let strike_acc = test_env
        .ctx
        .banks_client
        .get_account(strike_rec_pub.clone())
        .await
        .unwrap()
        .unwrap();
    let asset_acc = test_env
        .ctx
        .banks_client
        .get_account(asset_rec_pub.clone())
        .await
        .unwrap()
        .unwrap();
    let strike_acc_balance = spl_token::state::Account::unpack_from_slice(&strike_acc.data[..])
        .unwrap()
        .amount;
    let asset_acc_balance = spl_token::state::Account::unpack_from_slice(&asset_acc.data[..])
        .unwrap()
        .amount;

    let asset_transferred = asset_acc_balance - asset_init_balance;
    let strike_transferred = strike_acc_balance - strike_init_balance;

    assert_eq!(
        asset_transferred, contract_pda.contract_data.token_qty,
        "incorrect amount of asset transferred"
    );
    assert_eq!(
        strike_transferred, contract_pda.contract_data.strike_qty,
        "incorrect strike amount transferred"
    );

    let buyer_holder_balance = get_token_balance(test_env, test_env.buyer_holder.clone()).await;

    assert_eq!(
        buyer_holder_balance, 0,
        "incorrect buyer holder token balance"
    );
}

async fn cancel_offer(test_env: &mut TestEnv, contract_pda: ContractPDA, init_mode: &InitMode) {
    let (pda, _bump) = Pubkey::find_program_address(
        &[&contract_pda.seed, &contract_pda.index_seed],
        &test_env.program_key.pubkey(),
    );

    let (initialiser, token_temp, token_ata) = match init_mode {
        InitMode::BUYER => (
            &test_env.buyer.main,
            test_env.buyer_temp.clone(),
            test_env.writer.mint_2.clone(),
        ),
        InitMode::WRITER => (
            &test_env.writer.main,
            test_env.writer_temp.clone(),
            test_env.buyer.mint_1.clone(),
        ),
    };

    let accounts = vec![
        AccountMeta {
            pubkey: initialiser.pubkey(),
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: token_temp,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: pda,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: system_program::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: spl_token::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    println!("sending cancel_offer transaction...");
    let instruction_data = &[4];
    let ix = Instruction::new_with_bytes(test_env.program_key.pubkey(), instruction_data, accounts);
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, initialiser],
        test_env.ctx.last_blockhash,
    );
    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    test_env
        .ctx
        .banks_client
        .get_account(pda)
        .await
        .unwrap()
        .ok_or("")
        .expect_err("PDA account not closed");

    println!("sending tokens to initialiser ATA");
    let ix = spl_token::instruction::transfer(
        &spl_token::id(),
        &token_temp,
        &token_ata,
        &initialiser.pubkey(),
        &[&initialiser.pubkey()],
        5,
    )
    .unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, initialiser],
        test_env.ctx.last_blockhash,
    );
    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    println!("closing initialiser temp");
    let ix = spl_token::instruction::close_account(
        &spl_token::id(),
        &token_temp,
        &initialiser.pubkey(),
        &initialiser.pubkey(),
        &[&initialiser.pubkey()],
    )
    .unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, initialiser],
        test_env.ctx.last_blockhash,
    );
    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    let buyer_holder_balance = get_token_balance(test_env, test_env.buyer_holder.clone()).await;

    assert_eq!(
        buyer_holder_balance, 0,
        "incorrect buyer holder token balance"
    );
}

async fn expire_contract(test_env: &mut TestEnv, contract_pda: ContractPDA) {
    let (pda, _bump) = Pubkey::find_program_address(
        &[&contract_pda.seed, &contract_pda.index_seed],
        &test_env.program_key.pubkey(),
    );

    let accounts = vec![
        AccountMeta {
            pubkey: test_env.writer.main.pubkey(),
            is_signer: true,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.writer_temp,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: pda,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: test_env.buyer.main.pubkey(),
            is_signer: false,
            is_writable: true,
        },
        AccountMeta {
            pubkey: system_program::id(),
            is_signer: false,
            is_writable: false,
        },
        AccountMeta {
            pubkey: spl_token::id(),
            is_signer: false,
            is_writable: false,
        },
    ];

    let instruction_data = &[6];

    println!("sending expire_contract instruction");
    let ix = Instruction::new_with_bytes(test_env.program_key.pubkey(), instruction_data, accounts);
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, &test_env.writer.main],
        test_env.ctx.last_blockhash,
    );

    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    test_env
        .ctx
        .banks_client
        .get_account(pda)
        .await
        .unwrap()
        .ok_or("")
        .expect_err("PDA account not closed");

    println!("sending tokens to writer ATA");
    let ix = spl_token::instruction::transfer(
        &spl_token::id(),
        &test_env.writer_temp,
        &test_env.writer.mint_1,
        &test_env.writer.main.pubkey(),
        &[&test_env.writer.main.pubkey()],
        5,
    )
    .unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, &test_env.writer.main],
        test_env.ctx.last_blockhash,
    );
    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    println!("closing writer temp");
    let ix = spl_token::instruction::close_account(
        &spl_token::id(),
        &test_env.writer_temp,
        &test_env.writer.main.pubkey(),
        &test_env.writer.main.pubkey(),
        &[&test_env.writer.main.pubkey()],
    )
    .unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&test_env.ctx.payer.pubkey()),
        &[&test_env.ctx.payer, &test_env.writer.main],
        test_env.ctx.last_blockhash,
    );
    test_env
        .ctx
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();
}

async fn get_token_balance(test_env: &mut TestEnv, pk: Pubkey) -> u64 {
    let acc = test_env
        .ctx
        .banks_client
        .get_account(pk)
        .await
        .unwrap()
        .expect("could not find account");

    let info = spl_token::state::Account::unpack_from_slice(&acc.data[..]).unwrap();
    info.amount
}
