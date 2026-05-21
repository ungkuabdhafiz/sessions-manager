use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use chrono::DateTime;
use serde_json::Value;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
struct Session {
    provider: String,
    id: String,
    cwd: PathBuf,
    timestamp: Option<String>,
    branch: Option<String>,
    touched_files: HashSet<String>,
    commands: Vec<String>,
    log_file_paths: Vec<PathBuf>,
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();

    // Check for help command
    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_help();
        return;
    }

    let include_subagents = args.contains(&"--include-subagents".to_string());
    args.retain(|a| a != "--include-subagents");

    let mut keyword_filter = None;
    if let Some(idx) = args.iter().position(|a| a == "--keyword") {
        if idx + 1 < args.len() {
            keyword_filter = Some(args[idx + 1].clone());
            args.remove(idx + 1);
        }
        args.remove(idx);
    }

    // Determine subcommands: "list" is default, or "show <session_id>"
    if args.len() > 1 && args[1] == "show" {
        if args.len() < 3 {
            eprintln!("Error: Please provide a session ID to show. Run `sessions show <session_id>`");
            return;
        }
        let session_id = &args[2];
        show_session(session_id);
        return;
    }

    // Default to "list" command
    // Check if "list" keyword was explicitly passed or omitted
    let mut filter_arg = None;
    if args.len() > 1 {
        if args[1] == "list" {
            if args.len() > 2 {
                filter_arg = Some(args[2].clone());
            }
        } else {
            filter_arg = Some(args[1].clone());
        }
    }

    list_sessions(filter_arg, include_subagents, keyword_filter);
}

fn print_help() {
    println!("\x1b[1mSessions Manager CLI\x1b[0m");
    println!("A tool to list and inspect AI sessions across providers (Claude, Gemini, Codex, Pi).");
    println!();
    println!("\x1b[1mUSAGE:\x1b[0m");
    println!("  sessions                     List sessions for the current directory");
    println!("  sessions <dir>               List sessions for the specified directory");
    println!("  sessions <file>              List sessions in current directory that touched <file>");
    println!("  sessions list <file/dir>     Explicit form of the list command");
    println!("  sessions show <session_id>   Show detailed information for a specific session");
    println!("  sessions -h, --help          Print this help message");
}

fn get_home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/hafiz"))
}

fn list_sessions(filter_arg: Option<String>, include_subagents: bool, keyword_filter: Option<String>) {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    
    // Parse target directory and file filter from the filter argument
    let mut target_dir = current_dir.clone();
    let mut file_filter: Option<String> = None;

    if let Some(ref arg) = filter_arg {
        let path = Path::new(arg);
        if path.is_dir() {
            if let Ok(abs_dir) = path.canonicalize() {
                target_dir = abs_dir;
            } else {
                target_dir = path.to_path_buf();
            }
        } else if path.is_file() {
            if let Ok(abs_file) = path.canonicalize() {
                if let Some(parent) = abs_file.parent() {
                    target_dir = parent.to_path_buf();
                }
                file_filter = abs_file.file_name().map(|n| n.to_string_lossy().to_string());
            } else {
                file_filter = Some(arg.clone());
            }
        } else {
            // Check if it's a relative file that doesn't exist yet but has a parent
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && parent.is_dir() {
                    if let Ok(abs_parent) = parent.canonicalize() {
                        target_dir = abs_parent;
                    }
                }
            }
            file_filter = Some(path.file_name().unwrap_or_else(|| path.as_os_str()).to_string_lossy().to_string());
        }
    }

    // Canonicalize target_dir for robust comparison
    if let Ok(abs_dir) = target_dir.canonicalize() {
        target_dir = abs_dir;
    }

    println!("\x1b[90mScanning all sessions for workspace: \x1b[37m\x1b[1m{}\x1b[0m", target_dir.display());
    if let Some(ref f) = file_filter {
        println!("\x1b[90mFiltering for sessions touching file: \x1b[32m\x1b[1m{}\x1b[0m", f);
    }
    if let Some(ref k) = keyword_filter {
        println!("\x1b[90mFiltering for sessions containing keyword: \x1b[33m\x1b[1m{}\x1b[0m", k);
    }
    println!();

    let sessions = scan_all_sessions(&target_dir, include_subagents);

    if sessions.is_empty() {
        println!("No sessions found for this workspace.");
        return;
    }

    // Filter by file if requested
    let mut filtered_sessions = sessions;
    if let Some(ref f) = file_filter {
        filtered_sessions = filtered_sessions
            .into_iter()
            .filter(|s| {
                s.touched_files.iter().any(|file| file.contains(f)) ||
                s.commands.iter().any(|cmd| cmd.contains(f))
            })
            .collect();
    }

    if let Some(ref kw) = keyword_filter {
        let kw_lower = kw.to_lowercase();
        filtered_sessions.retain(|s| {
            for log_path in &s.log_file_paths {
                if let Ok(file) = File::open(log_path) {
                    let reader = BufReader::new(file);
                    for line in reader.lines().map_while(Result::ok) {
                        if line.to_lowercase().contains(&kw_lower) {
                            return true;
                        }
                    }
                }
            }
            false
        });
    }

    if filtered_sessions.is_empty() {
        if let Some(ref f) = file_filter {
            println!("No sessions matched the file filter '{}'.", f);
        } else if let Some(ref k) = keyword_filter {
            println!("No sessions matched the keyword filter '{}'.", k);
        } else {
            println!("No sessions matched the provided filters.");
        }
        return;
    }

    // Sort by timestamp descending
    filtered_sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // Print headers
    println!(
        "\x1b[1m{:<10} {:<38} {:<22} {:<12} {}\x1b[0m",
        "Provider", "Session ID", "Last Active", "Branch", "Touched Files"
    );
    println!("{}", "\x1b[90m-\x1b[0m".repeat(100));

    for s in &filtered_sessions {
        let provider_color = match s.provider.as_str() {
            "Claude" => "\x1b[36m", // Cyan
            "Gemini" => "\x1b[35m", // Purple
            "Gemini CLI" => "\x1b[95m", // Pinkish Purple
            "Codex" => "\x1b[32m",  // Green
            "Pi" => "\x1b[33m",     // Yellow
            _ => "\x1b[37m",
        };

        let last_active = s.timestamp.as_deref().unwrap_or("Unknown");
        // format ISO timestamp to cleaner readable local format
        let clean_time = format_timestamp(last_active);
        
        let branch = s.branch.as_deref().unwrap_or("-");

        // Format touched files to relative path and join
        let files: Vec<String> = s.touched_files.iter()
            .map(|f| {
                let p = Path::new(f);
                if p.is_absolute() {
                    if let Ok(rel) = p.strip_prefix(&target_dir) {
                        rel.to_string_lossy().to_string()
                    } else {
                        p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| f.clone())
                    }
                } else {
                    f.clone()
                }
            })
            .collect();

        let files_str = if files.is_empty() {
            "\x1b[90mNone\x1b[0m".to_string()
        } else {
            files.join(", ")
        };

        // Truncate files string to fit screen nicely
        let truncated_files = if files_str.len() > 30 {
            format!("{}{}", &files_str[..27], "...")
        } else {
            files_str
        };

        println!(
            "{}{:<10}\x1b[0m {:<38} {:<22} {:<12} {}",
            provider_color, s.provider, s.id, clean_time, branch, truncated_files
        );
    }
    println!("{}", "\x1b[90m-\x1b[0m".repeat(100));
    println!("\x1b[90mTotal sessions found: {}\x1b[0m", filtered_sessions.len());
    println!("\x1b[90mRun `sessions show <session_id>` to view full details (files touched, commands run).\x1b[0m");
}

fn format_timestamp(ts: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else if let Ok(dt) = DateTime::parse_from_str(ts, "%Y-%m-%dT%H-%M-%SZ") {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        ts.to_string()
    }
}

fn show_session(session_id: &str) {
    println!("\x1b[90mSearching for session ID: \x1b[37m\x1b[1m{}\x1b[0m", session_id);
    println!();

    // We scan all sessions globally (without filtering by cwd) to find the ID
    let home = get_home_dir();
    let mut all_sessions = Vec::new();
    
    // Scan all paths
    scan_claude(&home, None, &mut all_sessions, true);
    scan_gemini(&home, None, &mut all_sessions);
    scan_old_gemini(&home, None, &mut all_sessions);
    scan_codex(&home, None, &mut all_sessions);
    scan_pi(&home, None, &mut all_sessions);

    let all_sessions = merge_sessions(all_sessions);
    let session = all_sessions.into_iter().find(|s| s.id.starts_with(session_id) || session_id.starts_with(&s.id));

    match session {
        Some(s) => {
            let provider_color = match s.provider.as_str() {
                "Claude" => "\x1b[36m",
                "Gemini" => "\x1b[35m",
                "Gemini CLI" => "\x1b[95m",
                "Codex" => "\x1b[32m",
                "Pi" => "\x1b[33m",
                _ => "\x1b[37m",
            };

            println!("==========================================================================================");
            println!("{} SESSION DETAILS\x1b[0m", s.provider.to_uppercase());
            println!("==========================================================================================");
            println!("\x1b[1mSession ID:\x1b[0m     {}", s.id);
            println!("\x1b[1mProvider:\x1b[0m       {}{}\x1b[0m", provider_color, s.provider);
            println!("\x1b[1mWorkspace CWD:\x1b[0m  {}", s.cwd.display());
            println!("\x1b[1mLast Active:\x1b[0m    {}", s.timestamp.as_deref().unwrap_or("Unknown"));
            if let Some(ref b) = s.branch {
                println!("\x1b[1mGit Branch:\x1b[0m     {}", b);
            }
            println!("\x1b[1mLog Files ({}):\x1b[0m", s.log_file_paths.len());
            for path in &s.log_file_paths {
                println!("                {}", path.display());
            }
            println!("------------------------------------------------------------------------------------------");
            
            println!("\x1b[1mFILES TOUCHED ({}):\x1b[0m", s.touched_files.len());
            if s.touched_files.is_empty() {
                println!("  None");
            } else {
                let mut files: Vec<&String> = s.touched_files.iter().collect();
                files.sort();
                for f in files {
                    println!("  \x1b[32m•\x1b[0m {}", f);
                }
            }
            
            println!("------------------------------------------------------------------------------------------");
            println!("\x1b[1mCOMMANDS RUN ({}):\x1b[0m", s.commands.len());
            if s.commands.is_empty() {
                println!("  None");
            } else {
                for cmd in &s.commands {
                    println!("  \x1b[33m$\x1b[0m {}", cmd);
                }
            }
            println!("==========================================================================================");
        }
        None => {
            println!("Error: Session with ID '{}' not found.", session_id);
        }
    }
}

fn scan_all_sessions(target_cwd: &Path, include_subagents: bool) -> Vec<Session> {
    let home = get_home_dir();
    let mut sessions = Vec::new();

    scan_claude(&home, Some(target_cwd), &mut sessions, include_subagents);
    scan_gemini(&home, Some(target_cwd), &mut sessions);
    scan_old_gemini(&home, Some(target_cwd), &mut sessions);
    scan_codex(&home, Some(target_cwd), &mut sessions);
    scan_pi(&home, Some(target_cwd), &mut sessions);

    merge_sessions(sessions)
}

fn merge_sessions(sessions: Vec<Session>) -> Vec<Session> {
    use std::collections::HashMap;
    let mut merged: HashMap<(String, String), Session> = HashMap::new();

    for s in sessions {
        let key = (s.provider.clone(), s.id.clone());
        if let Some(existing) = merged.get_mut(&key) {
            // Update timestamp if newer
            if let Some(ts) = &s.timestamp {
                if existing.timestamp.is_none() || ts > existing.timestamp.as_ref().unwrap() {
                    existing.timestamp = Some(ts.clone());
                }
            }
            // Combine files and commands
            existing.touched_files.extend(s.touched_files);
            existing.commands.extend(s.commands);
            // Combine log files
            existing.log_file_paths.extend(s.log_file_paths);
            
            // Re-sort and deduplicate log files
            existing.log_file_paths.sort();
            existing.log_file_paths.dedup();
            
            // Deduplicate commands
            let mut unique_cmds = HashSet::new();
            existing.commands.retain(|c| unique_cmds.insert(c.clone()));
        } else {
            merged.insert(key, s);
        }
    }

    merged.into_values().collect()
}

// Canonicalize paths inside parsed session
fn clean_path(path_str: &str, cwd: &Path) -> String {
    let path_str = path_str.trim().replace('"', "").replace('\\', "/");
    let p = Path::new(&path_str);
    if p.is_absolute() {
        p.to_string_lossy().to_string()
    } else {
        cwd.join(p).to_string_lossy().to_string()
    }
}

fn scan_claude(home: &Path, target_cwd: Option<&Path>, sessions: &mut Vec<Session>, include_subagents: bool) {
    let claude_projects = home.join(".claude/projects");
    if !claude_projects.is_dir() {
        return;
    }

    for entry in WalkDir::new(claude_projects)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !include_subagents && path.components().any(|c| c.as_os_str() == "subagents") {
            continue;
        }
        if path.is_file() && path.extension().map_or(false, |ext| ext == "jsonl") {
            if let Ok(session) = parse_claude_file(path) {
                if target_cwd.map_or(true, |target| is_in_workspace(&session.cwd, target)) {
                    sessions.push(session);
                }
            }
        }
    }
}

fn parse_claude_file(path: &Path) -> Result<Session, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut session_id = path.file_stem().map_or("unknown", |s| s.to_str().unwrap_or("unknown")).to_string();
    let mut cwd = PathBuf::new();
    let mut timestamp = None;
    let mut branch = None;
    let mut touched_files = HashSet::new();
    let mut commands = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            // Get session info
            if let Some(id) = v.get("sessionId").and_then(|id| id.as_str()) {
                session_id = id.to_string();
            }
            if let Some(cwd_str) = v.get("cwd").and_then(|c| c.as_str()) {
                cwd = PathBuf::from(cwd_str);
            }
            if let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()) {
                timestamp = Some(ts.to_string());
            }
            if let Some(br) = v.get("gitBranch").and_then(|b| b.as_str()) {
                branch = Some(br.to_string());
            }

            // Get tools
            if let Some(message) = v.get("message") {
                if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            let tool_name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let input = block.get("input");
                            
                            if tool_name == "Read" && input.is_some() {
                                if let Some(file_path) = input.unwrap().get("file_path").and_then(|f| f.as_str()) {
                                    touched_files.insert(clean_path(file_path, &cwd));
                                }
                            } else if tool_name == "Bash" && input.is_some() {
                                if let Some(cmd) = input.unwrap().get("command").and_then(|c| c.as_str()) {
                                    commands.push(cmd.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Session {
        provider: "Claude".to_string(),
        id: session_id,
        cwd,
        timestamp,
        branch,
        touched_files,
        commands,
        log_file_paths: vec![path.to_path_buf()],
    })
}

fn scan_gemini(home: &Path, target_cwd: Option<&Path>, sessions: &mut Vec<Session>) {
    let antigravity_dir = home.join(".gemini/antigravity-cli");
    let gemini_brain = antigravity_dir.join("brain");
    if !gemini_brain.is_dir() {
        return;
    }

    // Load workspace mapping from history.jsonl
    let mut history_map = std::collections::HashMap::new();
    let history_file = antigravity_dir.join("history.jsonl");
    if history_file.is_file() {
        if let Ok(file) = File::open(history_file) {
            let reader = BufReader::new(file);
            for line in reader.lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if let (Some(id), Some(ws)) = (
                        v.get("conversationId").and_then(|i| i.as_str()),
                        v.get("workspace").and_then(|w| w.as_str()),
                    ) {
                        history_map.insert(id.to_string(), PathBuf::from(ws));
                    }
                }
            }
        }
    }

    for entry in WalkDir::new(gemini_brain)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.file_name().map_or(false, |name| name == "transcript.jsonl") {
            if let Ok(session) = parse_gemini_file(path, &history_map) {
                if target_cwd.map_or(true, |target| is_in_workspace(&session.cwd, target)) {
                    sessions.push(session);
                }
            }
        }
    }
}

fn parse_gemini_file(path: &Path, history_map: &std::collections::HashMap<String, PathBuf>) -> Result<Session, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    // Session ID is the parent of parent folder name: .../brain/<uuid>/.system_generated/logs/transcript.jsonl
    let mut session_id = "unknown".to_string();
    if let Some(parent) = path.parent() { // logs
        if let Some(parent) = parent.parent() { // .system_generated
            if let Some(parent) = parent.parent() { // <uuid>
                session_id = parent.file_name().map_or("unknown", |n| n.to_str().unwrap_or("unknown")).to_string();
            }
        }
    }

    let mut cwd = history_map.get(&session_id).cloned().unwrap_or_default();
    let mut timestamp = None;
    let mut touched_files = HashSet::new();
    let mut commands = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            // Get timestamp
            if let Some(ts) = v.get("created_at").and_then(|t| t.as_str()) {
                timestamp = Some(ts.to_string());
            }

            // Extract cwd from nested objects if present
            if cwd.as_os_str().is_empty() {
                if let Some(turn_context) = v.get("turn_context") {
                    if let Some(cwd_str) = turn_context.get("cwd").and_then(|c| c.as_str()) {
                        cwd = PathBuf::from(cwd_str);
                    }
                }
            }

            // Extract tool calls
            if let Some(tool_calls) = v.get("tool_calls").and_then(|tc| tc.as_array()) {
                for tool in tool_calls {
                    let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let args = tool.get("args");

                    if args.is_some() {
                        let args_val = args.unwrap();
                        
                        // Sometimes cwd is passed inside tool calls
                        if cwd.as_os_str().is_empty() {
                            if let Some(cwd_str) = args_val.get("Cwd").and_then(|c| c.as_str()) {
                                cwd = PathBuf::from(cwd_str);
                            }
                        }

                        match name {
                            "write_to_file" | "replace_file_content" | "multi_replace_file_content" => {
                                if let Some(target) = args_val.get("TargetFile").and_then(|f| f.as_str()) {
                                    touched_files.insert(clean_path(target, &cwd));
                                }
                            }
                            "view_file" => {
                                if let Some(target) = args_val.get("AbsolutePath").and_then(|f| f.as_str()) {
                                    touched_files.insert(clean_path(target, &cwd));
                                }
                            }
                            "run_command" => {
                                if let Some(cmd) = args_val.get("CommandLine").and_then(|c| c.as_str()) {
                                    commands.push(cmd.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    Ok(Session {
        provider: "Gemini".to_string(),
        id: session_id,
        cwd,
        timestamp,
        branch: None, // Gemini logs don't directly record the branch in metadata
        touched_files,
        commands,
        log_file_paths: vec![path.to_path_buf()],
    })
}

fn scan_codex(home: &Path, target_cwd: Option<&Path>, sessions: &mut Vec<Session>) {
    let codex_sessions = home.join(".codex/sessions");
    if !codex_sessions.is_dir() {
        return;
    }

    for entry in WalkDir::new(codex_sessions)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "jsonl") {
            if let Ok(session) = parse_codex_file(path) {
                if target_cwd.map_or(true, |target| is_in_workspace(&session.cwd, target)) {
                    sessions.push(session);
                }
            }
        }
    }
}

fn parse_codex_file(path: &Path) -> Result<Session, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut session_id = "unknown".to_string();
    let mut cwd = PathBuf::new();
    let mut timestamp = None;
    let mut branch = None;
    let mut touched_files = HashSet::new();
    let mut commands = Vec::new();

    // Regexes for patch files in apply_patch input
    let add_file_re = regex::Regex::new(r"\*\*\* Add File:\s*(\S+)").unwrap();
    let patch_file_re = regex::Regex::new(r"\*\*\* Patch File:\s*(\S+)").unwrap();
    let edit_file_re = regex::Regex::new(r"\*\*\* Edit File:\s*(\S+)").unwrap();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            // Get timestamp
            if let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()) {
                timestamp = Some(ts.to_string());
            }

            // Session meta
            if v.get("type").and_then(|t| t.as_str()) == Some("session_meta") {
                if let Some(payload) = v.get("payload") {
                    if let Some(id) = payload.get("id").and_then(|i| i.as_str()) {
                        session_id = id.to_string();
                    }
                    if let Some(cwd_str) = payload.get("cwd").and_then(|c| c.as_str()) {
                        cwd = PathBuf::from(cwd_str);
                    }
                    if let Some(git_b) = payload.get("gitBranch").and_then(|g| g.as_str()) {
                        branch = Some(git_b.to_string());
                    }
                }
            }

            // Custom tool calls
            if let Some(payload) = v.get("payload") {
                if payload.get("type").and_then(|t| t.as_str()) == Some("custom_tool_call") {
                    let tool_name = payload.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    
                    if tool_name == "apply_patch" {
                        if let Some(input_str) = payload.get("input").and_then(|i| i.as_str()) {
                            for cap in add_file_re.captures_iter(input_str) {
                                touched_files.insert(clean_path(&cap[1], &cwd));
                            }
                            for cap in patch_file_re.captures_iter(input_str) {
                                touched_files.insert(clean_path(&cap[1], &cwd));
                            }
                            for cap in edit_file_re.captures_iter(input_str) {
                                touched_files.insert(clean_path(&cap[1], &cwd));
                            }
                        }
                    } else if tool_name == "exec_command" {
                        if let Some(cmd) = payload.get("input").and_then(|i| i.as_str()) {
                            commands.push(cmd.to_string());
                        }
                    }
                }
            }
        }
    }

    Ok(Session {
        provider: "Codex".to_string(),
        id: session_id,
        cwd,
        timestamp,
        branch,
        touched_files,
        commands,
        log_file_paths: vec![path.to_path_buf()],
    })
}

fn scan_pi(home: &Path, target_cwd: Option<&Path>, sessions: &mut Vec<Session>) {
    let pi_sessions = home.join(".pi/agent/sessions");
    if !pi_sessions.is_dir() {
        return;
    }

    for entry in WalkDir::new(pi_sessions)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "jsonl") {
            if let Ok(session) = parse_pi_file(path) {
                if target_cwd.map_or(true, |target| is_in_workspace(&session.cwd, target)) {
                    sessions.push(session);
                }
            }
        }
    }
}

fn parse_pi_file(path: &Path) -> Result<Session, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut session_id = "unknown".to_string();
    let mut cwd = PathBuf::new();
    let mut timestamp = None;
    let mut touched_files = HashSet::new();
    let mut commands = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            let type_str = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

            if type_str == "session" {
                if let Some(id) = v.get("id").and_then(|i| i.as_str()) {
                    session_id = id.to_string();
                }
                if let Some(cwd_str) = v.get("cwd").and_then(|c| c.as_str()) {
                    cwd = PathBuf::from(cwd_str);
                }
                if let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()) {
                    timestamp = Some(ts.to_string());
                }
            }

            if let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()) {
                timestamp = Some(ts.to_string());
            }

            // Extract tool calls from message role assistant content
            if let Some(message) = v.get("message") {
                if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) == Some("toolCall") {
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let args = block.get("arguments");

                            if args.is_some() {
                                let args_val = args.unwrap();
                                match name {
                                    "read" | "write" => {
                                        if let Some(p) = args_val.get("path").and_then(|p| p.as_str()) {
                                            touched_files.insert(clean_path(p, &cwd));
                                        }
                                    }
                                    "bash" => {
                                        if let Some(cmd) = args_val.get("command").and_then(|c| c.as_str()) {
                                            commands.push(cmd.to_string());
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Session {
        provider: "Pi".to_string(),
        id: session_id,
        cwd,
        timestamp,
        branch: None,
        touched_files,
        commands,
        log_file_paths: vec![path.to_path_buf()],
    })
}

fn is_in_workspace(p1: &Path, p2: &Path) -> bool {
    if p1.as_os_str().is_empty() || p2.as_os_str().is_empty() {
        return false;
    }
    let p1_canon = p1.canonicalize().unwrap_or_else(|_| p1.to_path_buf());
    let p2_canon = p2.canonicalize().unwrap_or_else(|_| p2.to_path_buf());
    p1_canon.starts_with(&p2_canon)
}

fn scan_old_gemini(home: &Path, target_cwd: Option<&Path>, sessions: &mut Vec<Session>) {
    let gemini_tmp = home.join(".gemini/tmp");
    if !gemini_tmp.is_dir() {
        return;
    }

    for entry in WalkDir::new(gemini_tmp)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "jsonl") {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if file_name.starts_with("session-") {
                if let Ok(session) = parse_old_gemini_file(path) {
                    if target_cwd.map_or(true, |target| is_in_workspace(&session.cwd, target)) {
                        sessions.push(session);
                    }
                }
            }
        }
    }
}

fn parse_old_gemini_file(path: &Path) -> Result<Session, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    // Get CWD from parent/.project_root
    let mut cwd = PathBuf::new();
    if let Some(parent) = path.parent() { // chats
        if let Some(parent) = parent.parent() { // <workspace_name>
            let project_root_file = parent.join(".project_root");
            if project_root_file.is_file() {
                if let Ok(content) = std::fs::read_to_string(project_root_file) {
                    cwd = PathBuf::from(content.trim());
                }
            }
        }
    }

    let mut session_id = path.file_stem().map_or("unknown", |s| s.to_str().unwrap_or("unknown")).to_string();
    let mut timestamp = None;
    let mut touched_files = HashSet::new();
    let mut commands = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            // Check sessionId
            if let Some(id) = v.get("sessionId").and_then(|s| s.as_str()) {
                session_id = id.to_string();
            }
            // Check startTime, etc.
            if let Some(ts) = v.get("startTime").and_then(|s| s.as_str()) {
                timestamp = Some(ts.to_string());
            }
            if let Some(ts) = v.get("lastUpdated").and_then(|s| s.as_str()) {
                timestamp = Some(ts.to_string());
            }
            if let Some(ts) = v.get("timestamp").and_then(|s| s.as_str()) {
                timestamp = Some(ts.to_string());
            }

            // Extract toolCalls
            if let Some(tool_calls) = v.get("toolCalls").and_then(|tc| tc.as_array()) {
                for tool in tool_calls {
                    let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let args = tool.get("args");

                    if args.is_some() {
                        let args_val = args.unwrap();
                        match name {
                            "read_file" | "write_file" | "edit_file" | "replace_file" | "view_file" => {
                                if let Some(target) = args_val.get("file_path").and_then(|f| f.as_str()) {
                                    touched_files.insert(clean_path(target, &cwd));
                                } else if let Some(target) = args_val.get("path").and_then(|f| f.as_str()) {
                                    touched_files.insert(clean_path(target, &cwd));
                                }
                            }
                            "bash" | "exec_command" | "run_command" => {
                                if let Some(cmd) = args_val.get("command").and_then(|c| c.as_str()) {
                                    commands.push(cmd.to_string());
                                } else if let Some(cmd) = args_val.get("cmd").and_then(|c| c.as_str()) {
                                    commands.push(cmd.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    Ok(Session {
        provider: "Gemini CLI".to_string(),
        id: session_id,
        cwd,
        timestamp,
        branch: None,
        touched_files,
        commands,
        log_file_paths: vec![path.to_path_buf()],
    })
}
