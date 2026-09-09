use anchor_lang::prelude::*;

use crate::{state::Escrow, ESCROW_SEED};

#[derive(Accounts)]
pub struct Update<'info> {
    pub maker: Signer<'info>,
    #[account(
        mut,
        has_one = maker,
        seeds = [ESCROW_SEED, maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump,
    )]
    pub escrow: Account<'info, Escrow>,
}

impl<'info> Update<'info> {
    /// Maker updates the ask amount and/or expiration without touching the vault.
    pub fn update(&mut self, receive: u64, expiration: i64) -> Result<()> {
        self.escrow.receive = receive;
        self.escrow.expiration = expiration;
        Ok(())
    }
}
