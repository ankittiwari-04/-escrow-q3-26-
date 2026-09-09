use {
    anchor_lang::{
        prelude::msg, solana_program::instruction::Instruction, solana_program::program_pack::Pack,
        system_program::ID as SYSTEM_PROGRAM_ID, AccountDeserialize, InstructionData,
        ToAccountMetas,
    },
    anchor_spl::{
        associated_token::{self, ID as ASSOCIATED_TOKEN_PROGRAM_ID},
        token::spl_token,
    },
    litesvm::LiteSVM,
    litesvm_token::{
        spl_token::ID as TOKEN_PROGRAM_ID, CreateAssociatedTokenAccount, CreateMint, MintTo,
    },
    solana_keypair::Keypair,
    solana_message::Message,
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

const DEPOSIT: u64 = 10_000_000;
const RECEIVE: u64 = 10_000_000;
const NEW_RECEIVE: u64 = 15_000_000;
const EXPIRATION: i64 = 17780206209;
const NEW_EXPIRATION: i64 = 18888888888;
const SEED: u64 = 123;

fn setup() -> (LiteSVM, Keypair) {
    let program_id = escrowq32026::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/escrowq32026.so"
    ));
    svm.add_program(program_id, bytes).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    (svm, payer)
}

fn send_ix(svm: &mut LiteSVM, ix: Instruction, signers: &[&Keypair]) {
    let payer = signers[0];
    let message = Message::new(&[ix], Some(&payer.pubkey()));
    let recent_blockhash = svm.latest_blockhash();
    let transaction = Transaction::new(signers, message, recent_blockhash);
    let tx = svm.send_transaction(transaction).unwrap();
    msg!("CUs Consumed: {}", tx.compute_units_consumed);
    msg!("Tx Signature: {}", tx.signature);
}

fn make_escrow(
    svm: &mut LiteSVM,
    maker: &Keypair,
    mint_a: Pubkey,
    mint_b: Pubkey,
    maker_ata_a: Pubkey,
    seed: u64,
    deposit: u64,
    receive: u64,
    expiration: i64,
) -> (Pubkey, Pubkey) {
    let maker_pk = maker.pubkey();
    let escrow = Pubkey::find_program_address(
        &[b"escrow", maker_pk.as_ref(), &seed.to_le_bytes()],
        &escrowq32026::id(),
    )
    .0;
    let vault = associated_token::get_associated_token_address(&escrow, &mint_a);

    let make_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Make {
            maker: maker_pk,
            mint_a,
            mint_b,
            maker_ata_a,
            escrow,
            vault,
            associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
            token_program: TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: escrowq32026::instruction::Make {
            deposit,
            seed,
            receive,
            expiration,
        }
        .data(),
    };

    send_ix(svm, make_ix, &[maker]);
    (escrow, vault)
}

#[test]
fn test_make_and_refund() {
    let (mut svm, payer) = setup();
    let maker = payer.pubkey();

    let mint_a = CreateMint::new(&mut svm, &payer)
        .decimals(6)
        .authority(&maker)
        .send()
        .unwrap();
    let mint_b = CreateMint::new(&mut svm, &payer)
        .decimals(6)
        .authority(&maker)
        .send()
        .unwrap();

    let maker_ata_a = CreateAssociatedTokenAccount::new(&mut svm, &payer, &mint_a)
        .owner(&maker)
        .send()
        .unwrap();

    MintTo::new(&mut svm, &payer, &mint_a, &maker_ata_a, 1000_000_000)
        .send()
        .unwrap();

    let (escrow, vault) = make_escrow(
        &mut svm,
        &payer,
        mint_a,
        mint_b,
        maker_ata_a,
        SEED,
        DEPOSIT,
        RECEIVE,
        EXPIRATION,
    );

    let vault_data =
        spl_token::state::Account::unpack(&svm.get_account(&vault).unwrap().data).unwrap();
    assert_eq!(vault_data.amount, DEPOSIT);
    assert_eq!(vault_data.owner, escrow);
    assert_eq!(vault_data.mint, mint_a);

    let escrow_data = escrowq32026::state::Escrow::try_deserialize(
        &mut svm.get_account(&escrow).unwrap().data.as_ref(),
    )
    .unwrap();
    assert_eq!(escrow_data.seed, SEED);
    assert_eq!(escrow_data.maker, maker);
    assert_eq!(escrow_data.mint_a, mint_a);
    assert_eq!(escrow_data.mint_b, mint_b);
    assert_eq!(escrow_data.receive, RECEIVE);

    msg!("\n\nMake successful — refunding");

    let refund_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Refund {
            maker,
            mint_a,
            maker_ata_a,
            escrow,
            vault,
            token_program: TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: escrowq32026::instruction::Refund {}.data(),
    };
    send_ix(&mut svm, refund_ix, &[&payer]);

    msg!("\n\nRefund successful");
    assert!(svm.get_account(&escrow).is_none());
    assert!(svm.get_account(&vault).is_none());

    let maker_ata_a_data =
        spl_token::state::Account::unpack(&svm.get_account(&maker_ata_a).unwrap().data).unwrap();
    assert_eq!(maker_ata_a_data.amount, 1000_000_000); // full balance restored
}

#[test]
fn test_make_update_and_take() {
    let (mut svm, payer) = setup();
    let maker_kp = &payer;
    let maker = maker_kp.pubkey();
    let taker_kp = Keypair::new();
    let taker = taker_kp.pubkey();
    svm.airdrop(&taker, 1_000_000_000).unwrap();

    let mint_a = CreateMint::new(&mut svm, maker_kp)
        .decimals(6)
        .authority(&maker)
        .send()
        .unwrap();
    let mint_b = CreateMint::new(&mut svm, maker_kp)
        .decimals(6)
        .authority(&maker)
        .send()
        .unwrap();

    let maker_ata_a = CreateAssociatedTokenAccount::new(&mut svm, maker_kp, &mint_a)
        .owner(&maker)
        .send()
        .unwrap();
    MintTo::new(&mut svm, maker_kp, &mint_a, &maker_ata_a, 1000_000_000)
        .send()
        .unwrap();

    let (escrow, vault) = make_escrow(
        &mut svm,
        maker_kp,
        mint_a,
        mint_b,
        maker_ata_a,
        SEED,
        DEPOSIT,
        RECEIVE,
        EXPIRATION,
    );

    // --- update: change receive + expiration; vault must stay untouched ---
    let update_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Update {
            maker,
            escrow,
        }
        .to_account_metas(None),
        data: escrowq32026::instruction::Update {
            receive: NEW_RECEIVE,
            expiration: NEW_EXPIRATION,
        }
        .data(),
    };
    send_ix(&mut svm, update_ix, &[maker_kp]);

    let escrow_data = escrowq32026::state::Escrow::try_deserialize(
        &mut svm.get_account(&escrow).unwrap().data.as_ref(),
    )
    .unwrap();
    assert_eq!(escrow_data.receive, NEW_RECEIVE);
    assert_eq!(escrow_data.expiration, NEW_EXPIRATION);

    let vault_before =
        spl_token::state::Account::unpack(&svm.get_account(&vault).unwrap().data).unwrap();
    assert_eq!(vault_before.amount, DEPOSIT);
    msg!("\n\nUpdate successful — vault still holds {}", DEPOSIT);

    // --- take: fund taker with mint_b, then complete the swap ---
    let taker_ata_b = CreateAssociatedTokenAccount::new(&mut svm, &taker_kp, &mint_b)
        .owner(&taker)
        .send()
        .unwrap();
    MintTo::new(&mut svm, maker_kp, &mint_b, &taker_ata_b, NEW_RECEIVE)
        .send()
        .unwrap();

    // init_if_needed destinations — pass derived ATAs (created during take if missing)
    let taker_ata_a = associated_token::get_associated_token_address(&taker, &mint_a);
    let maker_ata_b = associated_token::get_associated_token_address(&maker, &mint_b);

    let take_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Take {
            taker,
            maker,
            mint_a,
            mint_b,
            taker_ata_a,
            taker_ata_b,
            maker_ata_b,
            escrow,
            vault,
            associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
            token_program: TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: escrowq32026::instruction::Take {}.data(),
    };
    // taker must sign; maker is only a mutable account for rent
    send_ix(&mut svm, take_ix, &[&taker_kp]);

    msg!("\n\nTake successful");

    // Escrow + vault closed
    assert!(svm.get_account(&escrow).is_none());
    assert!(svm.get_account(&vault).is_none());

    // Taker received mint_a from vault
    let taker_ata_a_data =
        spl_token::state::Account::unpack(&svm.get_account(&taker_ata_a).unwrap().data).unwrap();
    assert_eq!(taker_ata_a_data.amount, DEPOSIT);

    // Maker received the updated mint_b ask
    let maker_ata_b_data =
        spl_token::state::Account::unpack(&svm.get_account(&maker_ata_b).unwrap().data).unwrap();
    assert_eq!(maker_ata_b_data.amount, NEW_RECEIVE);

    // Taker spent all their mint_b
    let taker_ata_b_data =
        spl_token::state::Account::unpack(&svm.get_account(&taker_ata_b).unwrap().data).unwrap();
    assert_eq!(taker_ata_b_data.amount, 0);
}
