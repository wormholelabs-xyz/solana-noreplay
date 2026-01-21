use pinocchio::{entrypoint, AccountView, Address, ProgramResult};

entrypoint!(process_instruction);

fn process_instruction(
    _program_id: &Address,
    _accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    Ok(())
}
