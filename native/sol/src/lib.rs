use std::rc::Rc;
use std::{str::FromStr, sync::Arc};
use std::thread;
use std::time::Duration;
use anyhow::{anyhow, Result};
use borsh::{BorshSerialize, BorshDeserialize};
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::{
     solana_sdk::{
        instruction::Instruction,
        message::Message,
        pubkey::Pubkey,
        transaction::Transaction,
        system_instruction::SystemInstruction,
        commitment_config::CommitmentConfig,
        signature::{Keypair, Signer, Signature, read_keypair_file},
    },
    Client, Cluster, Program, ClientError
};
use mpl_candy_machine::{
    constants::FREEZE_FEATURE_INDEX, is_feature_active, CandyMachine, CandyMachineData, FreezePDA,
    WhitelistMintMode, WhitelistMintSettings,
};
use mpl_candy_machine::CollectionPDA;

use rust_base58::{ToBase58, FromBase58};

use bufstream::BufStream;
use rustler::types::OwnedBinary;
use rustler::{Atom, Env, Error, NifStruct, ResourceArc, Term};

pub type PdaInfo<T> = (Pubkey, T);

static CANDY_MACHINE_ID: &str = "664KTWjUbE9pJJ42imjiiFU8mwdgq1aTkesgDsofdg88";

mod atoms {
    rustler::atoms! {
        ok,
        error,
        eof,

        // Posix
        enoent, // File does not exist
        eacces, // Permission denied
        epipe, // Broken pipe
        eexist, // File exists

        unknown // Other error
    }
}

struct SignatureWrapper {
    pub signature: Signature,
}

fn load(env: Env, _: Term) -> bool {
    rustler::resource!(SignatureWrapper, env);
    true
}

// fn io_error_to_term(err: &IoError) -> Atom {
//     match err.kind() {
//         IoErrorKind::NotFound => atoms::enoent(),
//         IoErrorKind::PermissionDenied => atoms::eacces(),
//         IoErrorKind::BrokenPipe => atoms::epipe(),
//         IoErrorKind::AlreadyExists => atoms::eexist(),
//         _ => atoms::unknown(),
//     }
// }

#[rustler::nif]
fn add(a: i64, b: i64) -> i64 {
    a + b
}

// fn solana_error_to_term(err: &IoError) -> Atom {
//     match err.kind() {
//         IoErrorKind::NotFound => atoms::enoent(),
//         IoErrorKind::PermissionDenied => atoms::eacces(),
//         IoErrorKind::BrokenPipe => atoms::epipe(),
//         IoErrorKind::AlreadyExists => atoms::eexist(),
//         _ => atoms::unknown(),
//     }
// }

// TODO: Handle solana transaction error types
macro_rules! handle_solana_error {
    ($e:expr) => {
        match $e {
            Ok(inner) => inner,
            Err(ref error) => return error.to_string(),
        }
    };
}

// pub fn mint_nft<'info>(
//     ctx: Context<'_, '_, '_, 'info, MintNFT<'info>>,
//     creator_bump: u8
// ) -> Result<()>

pub fn get_candy_machine_state(
    client: &Client,
    candy_machine_id: &Pubkey,
) -> Result<CandyMachine> {
    let program = client.program(Pubkey::from_str(CANDY_MACHINE_ID).unwrap());

    program.account(*candy_machine_id).map_err(|e| match e {
        ClientError::AccountNotFound => anyhow!("Candy Machine does not exist!"),
        _ => anyhow!(
            "Failed to deserialize Candy Machine account {}: {}",
            candy_machine_id.to_string(),
            e
        ),
    })
}

pub fn find_collection_pda(candy_machine_id: &Pubkey) -> (Pubkey, u8) {
    // Derive collection PDA address
    let collection_seeds = &["collection".as_bytes(), candy_machine_id.as_ref()];

    Pubkey::find_program_address(collection_seeds, &Pubkey::from_str(CANDY_MACHINE_ID).unwrap())
}

pub fn get_collection_pda(
    candy_machine: &Pubkey,
    program: &Program,
) -> Result<PdaInfo<CollectionPDA>> {
    let collection_pda_pubkey = find_collection_pda(candy_machine).0;
    program
        .account(collection_pda_pubkey)
        .map(|c| (collection_pda_pubkey, c))
        .map_err(|e| match e {
            ClientError::AccountNotFound => anyhow!("Candy Machine collection is not set!"),
            _ => anyhow!(
                "Failed to deserialize collection PDA account: {}",
                &collection_pda_pubkey.to_string()
            ),
        })
}

fn mint_nft() -> String {
    let rpc_url = "https://api.devnet.solana.com".to_string();
    let ws_url = rpc_url.replace("http", "ws");
    let cluster = Cluster::Custom(rpc_url, ws_url);

    // TODO: Get sugar config keypair from env
    // let key_bytes = Keypair::new().to_bytes();
    // let signer = Rc::new(Keypair::from_bytes(&key_bytes));
    let signer = Rc::new(Keypair::from_bytes(&[0u8; 32]).unwrap());

    let opts = CommitmentConfig::confirmed();
    let client = Client::new_with_options(cluster, signer, opts);

    let candy_pubkey = Pubkey::from_str(CANDY_MACHINE_ID).unwrap();
    let candy_machine_state = Arc::new(get_candy_machine_state(&client, &candy_pubkey));

    let collection_pda_info =
        Arc::new(get_collection_pda(&candy_pubkey, &client.program(candy_pubkey)).ok());

    "Success".to_string()
}

#[rustler::nif]
fn send_initialize_tx() -> String {
    let client = RpcClient::new("https://api.devnet.solana.com".to_string());

    let payee = Keypair::new();
    let payer = Keypair::new();
    
    let payer_signature = client.request_airdrop(&payer.pubkey(), 1000000000).unwrap();
    let payee_signature = client.request_airdrop(&payee.pubkey(), 1000000000).unwrap();
    // Wait 10 seconds for airdrop confirmation
    thread::sleep(Duration::from_millis(15000));

    let transferInstruction = anchor_client::solana_sdk::system_instruction::transfer(&payer.pubkey(), &payee.pubkey(), 10);

    let blockhash = handle_solana_error!(client.get_latest_blockhash());
    let mut tx = Transaction::new_signed_with_payer(
        &[transferInstruction],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );

    let signature = handle_solana_error!(client.send_and_confirm_transaction(&tx));

    // TODO: Return a result type with signature and error
    // let resource = ResourceArc::new(SignatureWrapper {
    //     signature: signature,
    // });

    format!("You can view your transaction on the Solana Explorer at: https://explorer.solana.com/tx/{}?cluster=devnet", signature).to_string()
}

rustler::init!("Elixir.NifSol.Native", [add, send_initialize_tx], load = load);
