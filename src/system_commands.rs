use crate::search_mode::SearchResult;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

pub struct SystemCommand {
    pub name: String,
    pub command: String,
}

impl SystemCommand {
    pub fn new(name: &str, command: &str) -> Self {
        Self {
            name: name.to_string(),
            command: command.to_string(),
        }
    }
}

pub fn get_system_commands() -> Vec<SystemCommand> {
    vec![
        SystemCommand::new("Shutdown", "osascript -e 'tell app \"System Events\" to shut down'"),
        SystemCommand::new("Reboot", "osascript -e 'tell app \"System Events\" to restart'"),
        SystemCommand::new("Sleep", "osascript -e 'tell app \"System Events\" to sleep'"),
        SystemCommand::new("Lock Screen", "pmset displaysleepnow"),
    ]
}

pub fn search_commands(query: &str) -> Vec<SearchResult> {
    let commands = get_system_commands();

    // Show all commands if query is empty
    if query.is_empty() {
        return commands
            .into_iter()
            .map(|cmd| SearchResult::new(cmd.name, cmd.command, crate::search_mode::SearchMode::Run))
            .collect();
    }

    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<_> = commands
        .into_iter()
        .filter_map(|cmd| {
            matcher
                .fuzzy_match(&cmd.name, query)
                .map(|score| (cmd, score))
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored
        .into_iter()
        .map(|(cmd, _)| SearchResult::new(cmd.name, cmd.command, crate::search_mode::SearchMode::Run))
        .collect()
}
