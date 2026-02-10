use anchor_lang::prelude::*;

#[error_code]
pub enum EscrowError {
    #[msg("Escrow has not matured yet")]
    EscrowNotMature,

    #[msg("Vault token account is not owned by the escrow PDA")]
    InvalidVaultAuthority,

    #[msg("Vault is empty")]
    EmptyVault,

    #[msg("Invalid escrow account")]
    InvalidEscrow,
}
