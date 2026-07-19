fn unsafe_close(account: &AccountInfo) -> ProgramResult {
    if !account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }
    let _checked_example = account.lamports().checked_sub(1);
    **account.lamports.borrow_mut() = 0;
    Ok(())
}
