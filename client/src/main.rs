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
use makidex_amm::instruction::*;

#[derive(Clone, Debug, PartialEq)]
pub struct ClientConfig {
    // Global
    http_url: String,
    ws_url: String,
    payer_path: String,
    admin_path: String,
    withdrawer_path: String,
    admin_key: Pubkey,
    raydium_program: Pubkey,
    pnl_owner: Pubkey,
    withdrawer: Pubkey,

    // Withdraw
    amm_pool: Pubkey,
    amm_open_orders: Pubkey,
    amm_coin_vault: Pubkey,
    amm_pc_vault: Pubkey,
    amm_target_orders: Pubkey,
    coin_mint: Pubkey,
    pc_mint: Pubkey,
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
    let withdrawer_path = config.get("Global", "withdrawer_path").unwrap();
    if withdrawer_path.is_empty() {
        panic!("withdrawer_path must not be empty");
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

    let withdrawer_str = config.get("Global", "withdrawer").unwrap();
    if withdrawer_str.is_empty() {
        panic!("withdrawer must not be empty");
    }
    let withdrawer = Pubkey::from_str(&withdrawer_str).unwrap();

    let admin_key_str = config.get("Global", "admin_key").unwrap();
    if admin_key_str.is_empty() {
        panic!("admin_key must not be empty");
    }
    let admin_key = Pubkey::from_str(&admin_key_str).unwrap();
    
    let amm_pool_str = config.get("Withdraw", "amm_pool").unwrap();
    let mut amm_pool;
    if amm_pool_str.is_empty() {
        panic!("amm_pool must not be empty");
    } else {
        amm_pool = Pubkey::from_str(&amm_pool_str).unwrap();
    }

    let amm_open_orders_str = config.get("Withdraw", "amm_open_orders").unwrap();
    let mut amm_open_orders;
    if amm_open_orders_str.is_empty() {
        panic!("amm_open_orders must not be empty");
    } else {
        amm_open_orders = Pubkey::from_str(&amm_open_orders_str).unwrap();
    }

    let amm_coin_vault_str = config.get("Withdraw", "amm_coin_vault").unwrap();
    let mut amm_coin_vault;
    if amm_coin_vault_str.is_empty() {
        panic!("amm_coin_vault must not be empty");
    } else {
        amm_coin_vault = Pubkey::from_str(&amm_coin_vault_str).unwrap();
    }

    let amm_pc_vault_str = config.get("Withdraw", "amm_pc_vault").unwrap();
    let mut amm_pc_vault;
    if amm_pc_vault_str.is_empty() {
        panic!("amm_pc_vault must not be empty");
    } else {
        amm_pc_vault = Pubkey::from_str(&amm_pc_vault_str).unwrap();
    }

    let amm_target_orders_str = config.get("Withdraw", "amm_target_orders").unwrap();
    let mut amm_target_orders;
    if amm_target_orders_str.is_empty() {
        panic!("amm_target_orders must not be empty");
    } else {
        amm_target_orders = Pubkey::from_str(&amm_target_orders_str).unwrap();
    }

    let coin_mint_str = config.get("Withdraw", "coin_mint").unwrap();
    let mut coin_mint;
    if coin_mint_str.is_empty() {
        panic!("coin_mint must not be empty");
    } else {
        coin_mint = Pubkey::from_str(&coin_mint_str).unwrap();
    }

    let pc_mint_str = config.get("Withdraw", "pc_mint").unwrap();
    let mut pc_mint;
    if pc_mint_str.is_empty() {
        panic!("pc_mint must not be empty");
    } else {
        pc_mint = Pubkey::from_str(&pc_mint_str).unwrap();
    }

    Ok(ClientConfig {
        http_url,
        ws_url,
        payer_path,
        admin_path,
        withdrawer_path,
        admin_key,
        raydium_program,
        pnl_owner,
        withdrawer,

        amm_pool,
        amm_open_orders,
        amm_coin_vault,
        amm_pc_vault,
        amm_target_orders,
        coin_mint,
        pc_mint
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
    OwnerWithdrawPool {
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
    let withdrawer = read_keypair_file(&pool_config.withdrawer_path)?;
    let raydium_amm = pool_config.raydium_program;
    let pnl_owner = pool_config.pnl_owner;
    let admin_key = pool_config.admin_key;
    let amm_pool = pool_config.amm_pool;
    let amm_open_orders = pool_config.amm_open_orders;
    let amm_coin_vault = pool_config.amm_coin_vault;
    let amm_pc_vault = pool_config.amm_pc_vault;
    let amm_target_orders = pool_config.amm_target_orders;

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
                    &makidex_amm::processor::AMM_CONFIG_SEED
                ],
                &program.id(),
            );
        
            let create_instr = create_config_account(
                &raydium_amm,
                &admin_key, // &admin.pubkey(),
                &payer.pubkey(),
                &amm_config_key,
                &pnl_owner,
            )?;
            // send
            // let signers = vec![&payer, &admin];
            let signers = vec![&payer];
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
        CommandsName::OwnerWithdrawPool {
        } => {
            let program = anchor_client.program(pool_config.raydium_program)?;

            let (amm_authority_key, __bump) = Pubkey::find_program_address(
                &[
                    &makidex_amm::processor::AUTHORITY_AMM
                ],
                &program.id(),
            );
        
            // generate user_token_coin
            let user_token_coin_key = spl_associated_token_account::get_associated_token_address(&pool_config.withdrawer, &pool_config.coin_mint);

            // generate user_token_pc
            let user_token_pc_key = spl_associated_token_account::get_associated_token_address(&pool_config.withdrawer, &pool_config.pc_mint);

            let withdraw_instr = ownerwithdraw(
                &raydium_amm,
                &amm_pool,
                &amm_authority_key,
                &amm_open_orders,
                &pool_config.coin_mint,
                &pool_config.pc_mint,
                &amm_coin_vault,
                &amm_pc_vault,
                &user_token_coin_key,
                &user_token_pc_key,
                &pool_config.withdrawer, // &withdrawer.pubkey(),
                &amm_target_orders,
                &payer.pubkey(),
            )?;
            // send
            // let signers = vec![&payer, &admin];
            let signers = vec![&payer];
            let recent_hash = rpc_client.get_latest_blockhash()?;
            let txn = Transaction::new_signed_with_payer(
                &vec![withdraw_instr],
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
