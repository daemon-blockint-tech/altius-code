fn unchecked_authority(accounts: &[AccountInfo], program_id: &Pubkey) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let authority = next_account_info(account_info_iter)?;
    if authority.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    Ok(())
}
