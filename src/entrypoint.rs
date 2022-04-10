use crate::{
    instruction::{decode_instruction, InstructionType},
    processor,
};
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
};

entrypoint!(process_instruction);
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let action = decode_instruction(program_id, instruction_data)?;

    match action {
        InstructionType::Bid { instruction } => {
            return processor::initialise_contract(program_id, accounts, instruction)
        }
        InstructionType::Ask { instruction } => {
            return processor::initialise_contract(program_id, accounts, instruction)
        }
        InstructionType::AcceptBid => return processor::accept_bid(accounts),
        InstructionType::AcceptAsk => return processor::accept_ask(accounts),
        InstructionType::Execute => return processor::execute_contract(accounts),
        InstructionType::CancelOffer => return processor::cancel_offer(accounts),
        InstructionType::Expire => return processor::expire_contract(accounts),
    };
}
