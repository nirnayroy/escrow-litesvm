use crate::state::Escrow;
use crate::EscrowError;
use anchor_lang::prelude::Clock;
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};

const FIVE_DAYS: i64 = 5 * 24 * 60 * 60;

//Create context
#[derive(Accounts)]
pub struct Take<'info> {
    pub taker: Signer<'info>,

    /// CHECK: maker only used as seed + lamport destination
    pub maker: UncheckedAccount<'info>,

    pub mint_a: InterfaceAccount<'info, Mint>,
    pub mint_b: InterfaceAccount<'info, Mint>,

    #[account(mut)]
    pub taker_ata_a: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub taker_ata_b: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub maker_ata_b: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [
            b"escrow",
            escrow.maker.as_ref(),
            escrow.seed.to_le_bytes().as_ref(),
        ],
        bump = escrow.bump,
        close = maker,
    )]
    pub escrow: Account<'info, Escrow>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Take<'info> {
    pub fn take(&mut self) -> Result<()> {
        msg!("Instruction: Take");
        require!(
            self.escrow.maker != Pubkey::default(),
            EscrowError::InvalidEscrow
        );

        // ─────────────────────────────
        // 1. Enforce time lock
        // ─────────────────────────────
        let clock = Clock::get()?;
        msg!("Current ts: {}", clock.unix_timestamp);
        msg!("Escrow created_at: {}", self.escrow.created_at);

        require!(
            clock.unix_timestamp >= self.escrow.created_at + FIVE_DAYS,
            EscrowError::EscrowNotMature
        );

        // ─────────────────────────────
        // 2. Prepare PDA signer seeds (LIFETIME SAFE)
        // ─────────────────────────────

        msg!("Maker key: {}", self.escrow.maker);
        msg!("Escrow seed: {}", self.escrow.seed);
        msg!("Escrow bump: {}", self.escrow.bump);
        msg!("Escrow PDA: {}", self.escrow.key());

        let escrow_seeds: &[&[u8]] = &[
            b"escrow",
            self.escrow.maker.as_ref(),
            &self.escrow.seed.to_le_bytes(),
            &[self.escrow.bump],
        ];

        let signer_seeds: &[&[&[u8]]] = &[escrow_seeds];

        // ─────────────────────────────
        // 3. Sanity checks (prevent UB)
        // ─────────────────────────────
        require_keys_eq!(
            self.vault.owner,
            self.escrow.key(),
            EscrowError::InvalidVaultAuthority
        );

        msg!("Vault amount: {}", self.vault.amount);
        msg!("Escrow expects receive: {}", self.escrow.receive);

        let token_program = self.token_program.to_account_info();
        require!(
            self.vault.owner == self.escrow.key(),
            EscrowError::InvalidVaultAuthority
        );

        require!(self.vault.amount > 0, EscrowError::EmptyVault);

        // ─────────────────────────────
        // 4. Taker → Maker (mint_b)
        // ─────────────────────────────
        {
            msg!("Transferring mint_b from taker → maker");

            let cpi_accounts = TransferChecked {
                from: self.taker_ata_b.to_account_info(),
                to: self.maker_ata_b.to_account_info(),
                authority: self.taker.to_account_info(),
                mint: self.mint_b.to_account_info(),
            };

            let cpi_ctx = CpiContext::new(token_program.clone(), cpi_accounts);

            transfer_checked(cpi_ctx, self.escrow.receive, self.mint_b.decimals)?;
        }

        // ─────────────────────────────
        // 5. Vault → Taker (mint_a)
        // ─────────────────────────────
        {
            msg!("Transferring mint_a from vault → taker");

            let amount = self.vault.amount;
            require!(amount > 0, EscrowError::EmptyVault);

            let cpi_accounts = TransferChecked {
                from: self.vault.to_account_info(),
                to: self.taker_ata_a.to_account_info(),
                authority: self.escrow.to_account_info(),
                mint: self.mint_a.to_account_info(),
            };

            let cpi_ctx =
                CpiContext::new_with_signer(token_program.clone(), cpi_accounts, signer_seeds);

            transfer_checked(cpi_ctx, amount, self.mint_a.decimals)?;
        }

        // ─────────────────────────────
        // 6. Close vault (LAST use of escrow PDA)
        // ─────────────────────────────
        {
            msg!("Closing vault");

            let cpi_accounts = CloseAccount {
                account: self.vault.to_account_info(),
                destination: self.maker.to_account_info(),
                authority: self.escrow.to_account_info(),
            };

            let cpi_ctx = CpiContext::new_with_signer(token_program, cpi_accounts, signer_seeds);

            close_account(cpi_ctx)?;
        }

        // ─────────────────────────────
        // 7. Escrow auto-closes via `close = maker`
        // ─────────────────────────────
        msg!("Take completed successfully");

        Ok(())
    }
}
