fn checked_authority(
    accounts: &[AccountInfo],
    program_id: &Pubkey,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let authority = next_account_info(account_info_iter)?;
    if !authority.is_signer || authority.owner != program_id {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

fn checked_cpi(
    ix: &Instruction,
    accounts: &[AccountInfo],
    expected_program_id: &Pubkey,
) -> ProgramResult {
    if ix.program_id != *expected_program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    invoke(ix, accounts)
}

fn checked_close(account: &AccountInfo) -> ProgramResult {
    if !account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }
    let _checked_example = account.lamports().checked_sub(1);
    account.data.borrow_mut().fill(0);
    **account.lamports.borrow_mut() = 0;
    Ok(())
}
