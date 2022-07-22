// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use anyhow::{anyhow, bail, Result};
use clap::*;
use move_command_line_common::files::{MOVE_EXTENSION, MOVE_IR_EXTENSION};
use move_compiler::shared::NumericalAddress;
use move_core_types::{
    account_address::AccountAddress,
    identifier::Identifier,
    language_storage::{ModuleId, TypeTag},
    parser,
    transaction_argument::TransactionArgument,
};
use std::{fmt::Debug, path::Path, str::FromStr};
use tempfile::NamedTempFile;

#[derive(Debug)]
pub enum RawAddress {
    Named(Identifier),
    Anonymous(AccountAddress),
}

fn parse_address_literal(s: &str) -> Result<AccountAddress> {
    let (number, _number_format) = move_compiler::shared::parse_u128(s)
        .map_err(|e| anyhow!("Failed to parse address. Got error: {}", e))?;

    Ok(AccountAddress::new(number.to_be_bytes()))
}

impl RawAddress {
    pub fn parse(s: &str) -> Result<Self> {
        if let Ok(addr) = parse_address_literal(s) {
            return Ok(Self::Anonymous(addr));
        }
        let name =
            Identifier::new(s).map_err(|_| anyhow!("Failed to parse \"{}\" as address.", s))?;
        Ok(Self::Named(name))
    }
}

#[derive(Debug)]
pub struct LazyParseCommand<Command> {
    pub command_text: String,
    phantom: std::marker::PhantomData<Command>,
}

impl<Command> LazyParseCommand<Command>
where
    Command: Debug + Parser,
{
    pub fn new(command_text: String) -> Self {
        Self {
            command_text,
            phantom: std::marker::PhantomData,
        }
    }

    /// Parse the command text into the command, and render command text with jpst.
    pub fn parse(&self, ctx: &jpst::TemplateContext) -> Result<Command> {
        let command_text = jpst::format_str!(&self.command_text, ctx);
        let command_split = command_text.split_ascii_whitespace().collect::<Vec<_>>();

        let command = match Command::try_parse_from(command_split) {
            Ok(command) => command,
            Err(e) => {
                let mut spit_iter = command_text.split_ascii_whitespace();
                // skip 'task'
                spit_iter.next();
                let help_command = match spit_iter.next() {
                    None => vec!["task", "--help"],
                    Some(c) => vec!["task", c, "--help"],
                };
                let help = match Command::try_parse_from(help_command) {
                    Ok(_) => panic!(),
                    Err(e) => e,
                };
                bail!(
                    "Invalid command. Got error {}\nCommand {}.\n{}",
                    e,
                    command_text,
                    help
                )
            }
        };
        Ok(command)
    }
}

#[derive(Debug)]
pub struct LazyParseTaskInput<Command> {
    pub command: LazyParseCommand<Command>,
    pub name: String,
    pub number: usize,
    pub start_line: usize,
    pub command_lines_stop: usize,
    pub stop_line: usize,
    pub data: Option<NamedTempFile>,
}

impl<Command> LazyParseTaskInput<Command>
where
    Command: Debug + Parser,
{
    pub fn parse(self, ctx: &jpst::TemplateContext) -> Result<TaskInput<Command>> {
        let command = self.command.parse(ctx)?;
        let name = self.name;
        let number = self.number;
        let start_line = self.start_line;
        let command_lines_stop = self.command_lines_stop;
        let stop_line = self.stop_line;
        let data = self.data;
        Ok(TaskInput {
            command,
            name,
            number,
            start_line,
            command_lines_stop,
            stop_line,
            data,
        })
    }
}

#[derive(Debug)]
pub struct TaskInput<Command> {
    pub command: Command,
    pub name: String,
    pub number: usize,
    pub start_line: usize,
    pub command_lines_stop: usize,
    pub stop_line: usize,
    pub data: Option<NamedTempFile>,
}

pub fn taskify<Command: Debug + Parser>(
    filename: &Path,
) -> Result<Vec<LazyParseTaskInput<Command>>> {
    use regex::Regex;
    use std::{
        fs::File,
        io::{self, BufRead, Write},
    };
    #[allow(non_snake_case)]
    let WHITESPACE = Regex::new(r"^\s*$").unwrap();
    #[allow(non_snake_case)]
    let COMMAND_TEXT = Regex::new(r"^\s*//#\s*(.*)\s*$").unwrap();

    let file = File::open(filename).unwrap();
    let lines: Vec<String> = io::BufReader::new(file)
        .lines()
        .map(|ln| ln.expect("Could not parse line"))
        .collect();

    let lines_iter = lines.into_iter().enumerate().map(|(idx, l)| (idx + 1, l));
    let skipped_whitespace =
        lines_iter.skip_while(|(_line_number, line)| WHITESPACE.is_match(line));
    let mut bucketed_lines = vec![];
    let mut cur_commands = vec![];
    let mut cur_text = vec![];
    let mut in_command = true;
    for (line_number, line) in skipped_whitespace {
        if let Some(captures) = COMMAND_TEXT.captures(&line) {
            if !in_command {
                bucketed_lines.push((cur_commands, cur_text));
                cur_commands = vec![];
                cur_text = vec![];
                in_command = true;
            }
            let command_text = match captures.len() {
                1 => continue,
                2 => captures.get(1).unwrap().as_str().to_string(),
                n => panic!("COMMAND_TEXT captured {}. expected 1 or 2", n),
            };
            if command_text.is_empty() {
                continue;
            }
            cur_commands.push((line_number, command_text))
        } else if WHITESPACE.is_match(&line) {
            in_command = false;
            continue;
        } else {
            in_command = false;
            cur_text.push((line_number, line))
        }
    }
    bucketed_lines.push((cur_commands, cur_text));

    if bucketed_lines.is_empty() {
        return Ok(vec![]);
    }

    let mut tasks = vec![];
    for (number, (commands, text)) in bucketed_lines.into_iter().enumerate() {
        if commands.is_empty() {
            assert!(number == 0);
            bail!("No initial command")
        }

        let start_line = commands.first().unwrap().0;
        let command_lines_stop = commands.last().unwrap().0;
        let mut command_text = "task ".to_string();
        for (line_number, text) in commands {
            assert!(!text.is_empty(), "{}: {}", line_number, text);
            command_text = format!("{} {}", command_text, text);
        }
        let command_split = command_text.split_ascii_whitespace().collect::<Vec<_>>();
        let name = command_split
            .get(1)
            .map(|s| (*s).to_owned())
            .unwrap_or_else(|| format!("unknown_{}", number));
        let command = LazyParseCommand::new(command_text);

        let stop_line = if text.is_empty() {
            command_lines_stop
        } else {
            text[text.len() - 1].0
        };

        // Keep fucking this up somehow
        // let last_non_whitespace = text
        //     .iter()
        //     .rposition(|(_, l)| !WHITESPACE.is_match(l))
        //     .unwrap_or(0);
        // let initial_text = text
        //     .into_iter()
        //     .take_while(|(i, _)| *i < last_non_whitespace)
        //     .map(|(_, l)| l);
        let file_text_vec = (0..command_lines_stop)
            .map(|_| String::new())
            .chain(text.into_iter().map(|(_ln, l)| l))
            .collect::<Vec<String>>();
        let data = if file_text_vec.iter().all(|s| WHITESPACE.is_match(s)) {
            None
        } else {
            let data = NamedTempFile::new()?;
            data.reopen()?
                .write_all(file_text_vec.join("\n").as_bytes())?;
            Some(data)
        };

        tasks.push(LazyParseTaskInput {
            command,
            name,
            number,
            start_line,
            command_lines_stop,
            stop_line,
            data,
        })
    }
    Ok(tasks)
}

impl<T> TaskInput<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> TaskInput<U> {
        let Self {
            command,
            name,
            number,
            start_line,
            command_lines_stop,
            stop_line,
            data,
        } = self;
        TaskInput {
            command: f(command),
            name,
            number,
            start_line,
            command_lines_stop,
            stop_line,
            data,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxChoice {
    Source,
    IR,
}

/// When printing bytecode, the input program must either be a script or a module.
#[derive(Debug)]
pub enum PrintBytecodeInputChoice {
    Script,
    Module,
}

/// Translates the given Move IR module or script into bytecode, then prints a textual
/// representation of that bytecode.
#[derive(Debug, Parser)]
pub struct PrintBytecodeCommand {
    /// The kind of input: either a script, or a module.
    #[clap(long = "input", ignore_case = true, default_value = "script")]
    pub input: PrintBytecodeInputChoice,
}

#[derive(Debug, Parser)]
pub struct InitCommand {
    #[clap(
        long = "addresses",
        parse(try_from_str = move_compiler::shared::parse_named_address),
        takes_value(true),
        multiple_values(true),
        multiple_occurrences(true)
    )]
    pub named_addresses: Vec<(String, NumericalAddress)>,
}

#[derive(Debug, Parser)]
pub struct PublishCommand {
    #[clap(long = "gas-budget")]
    pub gas_budget: Option<u64>,
    #[clap(long = "syntax")]
    pub syntax: Option<SyntaxChoice>,
}

/// TODO: this is a hack to support named addresses in transaction argument positions.
/// Should reimplement in a better way in the future.
#[derive(Debug, PartialEq, Eq)]
pub enum Argument {
    NamedAddress(Identifier),
    TransactionArgument(TransactionArgument),
}

#[derive(Debug, Parser)]
pub struct RunCommand {
    #[clap(
        long = "signers",
        parse(try_from_str = RawAddress::parse),
        takes_value(true),
        multiple_values(true),
        multiple_occurrences(true)
    )]
    pub signers: Vec<RawAddress>,
    #[clap(
        long = "args",
        parse(try_from_str = parse_argument),
        takes_value(true),
        multiple_values(true),
        multiple_occurrences(true)
    )]
    pub args: Vec<Argument>,
    #[clap(
        long = "type-args",
        parse(try_from_str = parser::parse_type_tag),
        takes_value(true),
        multiple_values(true),
        multiple_occurrences(true)
    )]
    pub type_args: Vec<TypeTag>,
    #[clap(long = "gas-budget")]
    pub gas_budget: Option<u64>,
    #[clap(long = "syntax")]
    pub syntax: Option<SyntaxChoice>,
    #[clap(name = "NAME", parse(try_from_str = parse_qualified_module_access))]
    pub name: Option<(ModuleId, Identifier)>,
}

#[derive(Debug, Parser)]
pub struct ViewCommand {
    #[clap(long = "address", parse(try_from_str = RawAddress::parse))]
    pub address: RawAddress,
    #[clap(long = "resource", parse(try_from_str = parse_qualified_module_access_with_type_args))]
    pub resource: (ModuleId, Identifier, Vec<TypeTag>),
}

#[derive(Debug)]
pub enum TaskCommand<
    ExtraInitArgs: clap::Args,
    ExtraPublishArgs: clap::Args,
    ExtraRunArgs: clap::Args,
    SubCommands: clap::Args,
> {
    Init(InitCommand, ExtraInitArgs),
    PrintBytecode(PrintBytecodeCommand),
    Publish(PublishCommand, ExtraPublishArgs),
    Run(RunCommand, ExtraRunArgs),
    View(ViewCommand),
    Subcommand(SubCommands),
}

impl<
        ExtraInitArgs: clap::Args,
        ExtraPublishArgs: clap::Args,
        ExtraRunArgs: clap::Args,
        SubCommands: clap::Args,
    > FromArgMatches for TaskCommand<ExtraInitArgs, ExtraPublishArgs, ExtraRunArgs, SubCommands>
{
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, Error> {
        Ok(match matches.subcommand() {
            Some(("init", matches)) => TaskCommand::Init(
                FromArgMatches::from_arg_matches(matches)?,
                FromArgMatches::from_arg_matches(matches)?,
            ),
            Some(("print-bytecode", matches)) => {
                TaskCommand::PrintBytecode(FromArgMatches::from_arg_matches(matches)?)
            }
            Some(("publish", matches)) => TaskCommand::Publish(
                FromArgMatches::from_arg_matches(matches)?,
                FromArgMatches::from_arg_matches(matches)?,
            ),
            Some(("run", matches)) => TaskCommand::Run(
                FromArgMatches::from_arg_matches(matches)?,
                FromArgMatches::from_arg_matches(matches)?,
            ),
            Some(("view", matches)) => {
                TaskCommand::View(FromArgMatches::from_arg_matches(matches)?)
            }
            _ => TaskCommand::Subcommand(SubCommands::from_arg_matches(matches)?),
        })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl<
        ExtraInitArgs: clap::Args,
        ExtraPublishArgs: clap::Args,
        ExtraRunArgs: clap::Args,
        SubCommands: clap::Args,
    > CommandFactory for TaskCommand<ExtraInitArgs, ExtraPublishArgs, ExtraRunArgs, SubCommands>
{
    fn into_app<'help>() -> Command<'help> {
        let command = clap::Command::new("Task Command")
            .subcommand(ExtraInitArgs::augment_args(
                InitCommand::command().name("init"),
            ))
            .subcommand(PrintBytecodeCommand::command().name("print-bytecode"))
            .subcommand(ExtraPublishArgs::augment_args(
                PublishCommand::command().name("publish"),
            ))
            .subcommand(ExtraRunArgs::augment_args(
                RunCommand::command().name("run"),
            ))
            .subcommand(ViewCommand::command().name("view"));
        SubCommands::augment_args(command)
    }

    fn into_app_for_update<'help>() -> Command<'help> {
        todo!()
    }
}
// Note: this needs to be manually implemented because clap cannot handle generic tuples
// with more than 1 element currently.
//
// The code is a simplified version of what `#[derive(Parser)` would generate had it worked.
// (`cargo expand` is useful in printing out the derived code.)
//
impl<
        ExtraInitArgs: clap::Args,
        ExtraPublishArgs: clap::Args,
        ExtraRunArgs: clap::Args,
        SubCommands: clap::Args,
    > Parser for TaskCommand<ExtraInitArgs, ExtraPublishArgs, ExtraRunArgs, SubCommands>
{
}

#[derive(Debug, Parser)]
pub struct EmptyCommand {}

fn parse_qualified_module_access(s: &str) -> Result<(ModuleId, Identifier)> {
    match move_core_types::parser::parse_type_tag(s)? {
        TypeTag::Struct(s) => {
            let id = ModuleId::new(s.address, s.module);
            if !s.type_params.is_empty() {
                bail!("Invalid module access. Did not expect type arguments")
            }
            Ok((id, s.name))
        }
        _ => bail!("Invalid module access"),
    }
}

fn parse_qualified_module_access_with_type_args(
    s: &str,
) -> Result<(ModuleId, Identifier, Vec<TypeTag>)> {
    match move_core_types::parser::parse_type_tag(s)? {
        TypeTag::Struct(s) => {
            let id = ModuleId::new(s.address, s.module);
            Ok((id, s.name, s.type_params))
        }
        _ => bail!("Invalid module access"),
    }
}

impl FromStr for SyntaxChoice {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            MOVE_EXTENSION => Ok(SyntaxChoice::Source),
            MOVE_IR_EXTENSION => Ok(SyntaxChoice::IR),
            _ => Err(anyhow!(
                "Invalid syntax choice. Expected '{}' or '{}'",
                MOVE_EXTENSION,
                MOVE_IR_EXTENSION
            )),
        }
    }
}

impl FromStr for PrintBytecodeInputChoice {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "script" => Ok(PrintBytecodeInputChoice::Script),
            "module" => Ok(PrintBytecodeInputChoice::Module),
            _ => Err(anyhow!(
                "Invalid input choice. Expected 'script' or 'module'"
            )),
        }
    }
}

fn parse_argument(s: &str) -> Result<Argument> {
    Ok(match s.strip_prefix('@') {
        Some(stripped) => Argument::NamedAddress(Identifier::new(stripped)?),
        None => {
            let arg = match parser::parse_transaction_argument(s) {
                Ok(arg) => arg,
                Err(e) => {
                    //TODO migrate this to starcoin project after allow custom parse Argument
                    //auto covert 0xxx to vector<u8>
                    match s.strip_prefix("0x") {
                        Some(stripped) => TransactionArgument::U8Vector(hex::decode(stripped)?),
                        None => return Err(e),
                    }
                }
            };
            Argument::TransactionArgument(arg)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_parse_argument() {
        assert_eq!(
            parse_argument("@foo").unwrap(),
            Argument::NamedAddress(Identifier::new("foo").unwrap())
        );
        assert_eq!(
            parse_argument("0x80848150abee7e9a3bfe9542a019eb0b8b01f124b63b011f9c338fdb935c417d")
                .unwrap(),
            Argument::TransactionArgument(TransactionArgument::U8Vector(
                hex::decode("80848150abee7e9a3bfe9542a019eb0b8b01f124b63b011f9c338fdb935c417d")
                    .unwrap()
            ))
        );
    }
}
