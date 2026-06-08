use crate::rpc::types::RpcSlashCommand;
use crate::slash_commands::{slash_commands_to_rpc, SlashCommandInfo};

#[derive(Default)]
pub struct RpcCommandRegistry {
    slash_commands: Vec<RpcSlashCommand>,
}

impl RpcCommandRegistry {
    pub fn set_slash_commands(&mut self, commands: Vec<SlashCommandInfo>) {
        self.slash_commands = slash_commands_to_rpc(&commands);
    }

    pub fn set_rpc_slash_commands(&mut self, commands: Vec<RpcSlashCommand>) {
        self.slash_commands = commands;
    }

    pub fn rpc_slash_commands(&self) -> Vec<RpcSlashCommand> {
        self.slash_commands.clone()
    }
}
