#![allow(dead_code)]
use anchor_client::{Client, Cluster};
use anchor_lang::prelude::AccountMeta;
use anyhow::{format_err, Result};
use arrayref::array_ref;
use clap::Parser;
use configparser::ini::Ini;
use rand::rngs::OsRng;
use solana_account_decoder::{
    parse_token::{TokenAccountType, UiAccountState},
    UiAccountData, UiAccountEncoding,
};
use solana_client::{
    rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcTransactionConfig},
    rpc_filter::{Memcmp, RpcFilterType},
    rpc_request::TokenAccountsFilter,
};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    message::Message,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use solana_transaction_status::UiTransactionEncoding;
use std::path::Path;
use std::rc::Rc;
use std::str::FromStr;
use std::{collections::VecDeque, convert::identity, mem::size_of};

mod instructions;
use bincode::serialize;
use instructions::rpc::*;
use instructions::token_instructions::*;
use spl_associated_token_account::get_associated_token_address;
use spl_token_2022::{
    extension::StateWithExtensionsMut,
    state::Mint,
    state::{Account, AccountState},
};
use spl_token_client::token::ExtensionInitializationParams;
use raydium_amm::instruction::*;

#[derive(Clone, Debug, PartialEq)]
pub struct ClientConfig {
    http_url: String,
    ws_url: String,
    payer_path: String,
    admin_path: String,
    raydium_program: Pubkey,
    pnl_owner: Pubkey,
}

fn load_cfg(client_config: &String) -> Result<ClientConfig> {
    let mut config = Ini::new();
    let _map = config.load(client_config).unwrap();
    let http_url = config.get("Global", "http_url").unwrap();
    if http_url.is_empty() {
        panic!("http_url must not be empty");
    }
    let ws_url = config.get("Global", "ws_url").unwrap();
    if ws_url.is_empty() {
        panic!("ws_url must not be empty");
    }
    let payer_path = config.get("Global", "payer_path").unwrap();
    if payer_path.is_empty() {
        panic!("payer_path must not be empty");
    }
    let admin_path = config.get("Global", "admin_path").unwrap();
    if admin_path.is_empty() {
        panic!("admin_path must not be empty");
    }

    let raydium_program_str = config.get("Global", "raydium_program").unwrap();
    if raydium_program_str.is_empty() {
        panic!("raydium_program must not be empty");
    }
    let raydium_program = Pubkey::from_str(&raydium_program_str).unwrap();

    let pnl_owner_str = config.get("Global", "pnl_owner").unwrap();
    if pnl_owner_str.is_empty() {
        panic!("pnl_owner must not be empty");
    }
    let pnl_owner = Pubkey::from_str(&pnl_owner_str).unwrap();

    Ok(ClientConfig {
        http_url,
        ws_url,
        payer_path,
        admin_path,
        raydium_program,
        pnl_owner
    })
}
fn read_keypair_file(s: &str) -> Result<Keypair> {
    solana_sdk::signature::read_keypair_file(s)
        .map_err(|_| format_err!("failed to read keypair from {}", s))
}
fn write_keypair_file(keypair: &Keypair, outfile: &str) -> Result<String> {
    solana_sdk::signature::write_keypair_file(keypair, outfile)
        .map_err(|_| format_err!("failed to write keypair to {}", outfile))
}
fn path_is_exist(path: &str) -> bool {
    Path::new(path).exists()
}


#[derive(Debug, Parser)]
pub struct Opts {
    #[clap(subcommand)]
    pub command: CommandsName,
}
#[derive(Debug, Parser)]
pub enum CommandsName {
    CreateConfigAccount {
        // amm_program: Pubkey,
        // administrator: Pubkey,
        // amm_config: Pubkey,
        // pnl_owner: Pubkey,
    },
}
// #[cfg(not(feature = "async"))]
fn main() -> Result<()> {
    println!("Starting...");
    let client_config = "client_config.ini";
    let pool_config = load_cfg(&client_config.to_string()).unwrap();
    // Admin and cluster params.
    let payer = read_keypair_file(&pool_config.payer_path)?;
    let admin = read_keypair_file(&pool_config.admin_path)?;
    let raydium_amm = pool_config.raydium_program;
    let pnl_owner = pool_config.pnl_owner;

    // solana rpc client
    let rpc_client = RpcClient::new(pool_config.http_url.to_string());

    // anchor client.
    let anchor_config = pool_config.clone();
    let url = Cluster::Custom(anchor_config.http_url, anchor_config.ws_url);
    let wallet = read_keypair_file(&pool_config.payer_path)?;
    let anchor_client = Client::new(url, Rc::new(wallet));
    let program = anchor_client.program(pool_config.raydium_program)?;

    let opts = Opts::parse();
    match opts.command {
        CommandsName::CreateConfigAccount {
            // amm_program,
            // administrator,
            // amm_config,
            // pnl_owner,
        } => {
            let program = anchor_client.program(pool_config.raydium_program)?;
            let (amm_config_key, __bump) = Pubkey::find_program_address(
                &[
                    &raydium_amm::processor::AMM_CONFIG_SEED
                ],
                &program.id(),
            );
        
            let create_instr = create_config_account(
                &raydium_amm,
                &admin.pubkey(),
                &amm_config_key,
                &pnl_owner,
            )?;
            // send
            let signers = vec![&payer, &admin];
            let recent_hash = rpc_client.get_latest_blockhash()?;
            let txn = Transaction::new_signed_with_payer(
                &vec![create_instr],
                Some(&payer.pubkey()),
                &signers,
                recent_hash,
            );
            let signature = send_txn(&rpc_client, &txn, true)?;
            println!("{}", signature);
        }
    }

    Ok(())
}
