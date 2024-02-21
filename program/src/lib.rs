// #![deny(missing_docs)]

//! An Uniswap-like program for the Solana blockchain.
#[macro_use]
pub mod log;

mod entrypoint;
pub mod error;
pub mod instruction;
pub mod invokers;
pub mod math;
pub mod processor;
pub mod state;

// Export current solana-sdk types for downstream users who may also be building with a different solana-sdk version
use anchor_lang::prelude::*;

#[cfg(feature = "devnet")]
declare_id!("3Qvevpr9VQp7ECWjAU186oiSGjMhDucjU32oSX8BfxGK");
#[cfg(not(any(feature = "devnet", feature = "client")))]
declare_id!("3Qvevpr9VQp7ECWjAU186oiSGjMhDucjU32oSX8BfxGK");
