use std::thread;
use std::time::Duration;
use anyhow::Result;
use borsh::{BorshSerialize, BorshDeserialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
     instruction::Instruction,
     message::Message,
     pubkey::Pubkey,
     signature::{Keypair, Signer, Signature},
     transaction::Transaction,
     system_instruction::SystemInstruction
};
use rust_base58::{ToBase58, FromBase58};

use bufstream::BufStream;
use rustler::types::OwnedBinary;
use rustler::{Atom, Env, Error, NifStruct, ResourceArc, Term};

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

#[rustler::nif]
fn send_initialize_tx() -> String {
    let client = RpcClient::new("https://api.devnet.solana.com");
    let payee = Keypair::new();
    let payer = Keypair::new();
    
    let payer_signature = client.request_airdrop(&payer.pubkey(), 2000000000).unwrap();
    // Wait 10 seconds for airdrop confirmation
    thread::sleep(Duration::from_millis(15000));

    let transferInstruction = solana_sdk::system_instruction::transfer(&payer.pubkey(), &payee.pubkey(), 10);

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

    "You can view your transaction on the Solana Explorer at:\nhttps://explorer.solana.com/tx/#{signature}?cluster=devnet".to_string()
}

rustler::init!("Elixir.NifSol.Native", [add, send_initialize_tx], load = load);
