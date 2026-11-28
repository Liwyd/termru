//! A reader for OpenSSH client configuration (`~/.ssh/config`), for importing
//! hosts. **Pure**: it never touches the file system. The caller supplies the
//! text and an [`IncludeLoader`] that turns an `Include` pattern into file
//! contents (so a test, or a sandbox, decides what files exist).
//!
//! What is understood, and how it follows OpenSSH:
//!
//! * `Host` blocks with pattern lists (`*`, `?`, `!negation`). Every plain
//!   alias (no wildcard, not negated) is an import candidate; a block whose
//!   patterns are all wildcards (`Host *`, `Host *.example.com`) produces no
//!   host, but its options still apply to the hosts it matches — as defaults,
//!   or as overrides when it comes first.
//! * **First value wins**, per option, over the blocks that apply to a host
//!   in file order (`IdentityFile` is cumulative: every value counts, in
//!   order). Options before the first `Host` apply to every host.
//! * `HostName` (with `%h`), `User`, `Port`, `IdentityFile`, `ProxyJump`
//!   (a comma list of `[user@]host[:port]` or `ssh://` forms; `none` turns it
//!   off). `ProxyCommand` cannot be imported; a host that depends on one is
//!   reported with a warning.
//! * `Include`, resolved by the loader, spliced in place, nesting limited to
//!   [`MAX_INCLUDE_DEPTH`], a file never includes itself, at most
//!   [`MAX_FILES`] files in total.
//! * `Match` blocks are **skipped** (they need conditions this reader cannot
//!   evaluate) with a warning that names the place.
//! * `Keyword value`, `Keyword=value`, `Keyword = value`, double-quoted
//!   values, `#` comments; keywords are case-insensitive.
//!
//! Everything else is ignored silently: an SSH config holds a hundred options
//! that have no counterpart here.

/// Include files nest at most this deep (the file that is imported is depth 0).
pub const MAX_INCLUDE_DEPTH: usize = 8;
/// Files read in total (the root plus everything included).
pub const MAX_FILES: usize = 128;
/// Lines read in total; the rest is ignored with a warning. A config this
/// long is not hand-written.
pub const MAX_LINES: usize = 200_000;
/// Import candidates returned.
pub const MAX_CANDIDATES: usize = 2_000;

/// One file an `Include` resolved to.
#[derive(Debug, Clone)]
pub struct LoadedFile {
    /// How the file is named in warnings (typically its path).
    pub name: String,
    pub contents: String,
}

/// Turns the pattern of an `Include` into files. The parser does not know
/// where relative patterns are anchored (`~/.ssh`), what `~` means or how
/// wildcards expand: the caller does. Files are returned in the order they
/// are to be read (OpenSSH: sorted by name).
pub trait IncludeLoader {
    fn load(&mut self, pattern: &str) -> Vec<LoadedFile>;
}

/// A loader that finds nothing: for a configuration that must stand alone.
pub struct NoIncludes;

impl IncludeLoader for NoIncludes {
    fn load(&mut self, _pattern: &str) -> Vec<LoadedFile> {
        Vec::new()
    }
}

/// One entry of a `ProxyJump` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpRef {
    /// The host as written: an alias of this configuration, or a plain name.
    /// IPv6 literals are without brackets.
    pub host: String,
    /// `user@` and `:port` exactly as written in the list, when present.
    pub user: Option<String>,
    pub port: Option<u16>,
}

/// What the options that apply to one host add up to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effective {
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    /// Every `IdentityFile`, in order, as written (no `~` expansion).
    pub identity_files: Vec<String>,
    /// `None`: not set. `Some(vec![])`: `ProxyJump none`.
    pub proxy_jump: Option<Vec<JumpRef>>,
    /// The `ProxyCommand`, which is not importable.
    pub proxy_command: Option<String>,
    /// Problems found while reading the values of this host's options.
    pub warnings: Vec<String>,
}

/// A host the configuration declares, ready to be turned into a host of the
/// library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCandidate {
    /// The `Host` alias: what the user types after `ssh`. It becomes the
    /// name of the imported host.
    pub alias: String,
    /// `HostName` (with `%h` replaced), or the alias itself.
    pub host: String,
    /// `None` when no `User` applies: the importer picks a default.
    pub user: Option<String>,
    pub port: u16,
    /// Every `IdentityFile` that applies, in order, as written.
    pub identity_files: Vec<String>,
    /// The route, first hop first.
    pub jump_chain: Vec<JumpRef>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct Directive {
    keyword: String,
    args: Vec<String>,
    place: String,
}

#[derive(Debug, Clone)]
enum Scope {
    /// Before the first `Host`/`Match`: applies to every host.
    Global,
    Host(Vec<String>),
    /// Skipped: its conditions cannot be evaluated here.
    Match,
}

#[derive(Debug, Clone)]
struct Block {
    scope: Scope,
    directives: Vec<Directive>,
}

/// A parsed configuration.
#[derive(Debug, Clone)]
pub struct ParsedConfig {
    blocks: Vec<Block>,
    /// Things the importer should tell the user: skipped `Match` blocks,
    /// includes that could not be followed, unreadable values.
    pub warnings: Vec<String>,
}

/// Parses `text` (named `name` in warnings). Includes are resolved through
/// `loader`.
pub fn parse(name: &str, text: &str, loader: &mut dyn IncludeLoader) -> ParsedConfig {
    let mut parser = Parser {
        blocks: vec![Block {
            scope: Scope::Global,
            directives: Vec::new(),
        }],
        warnings: Vec::new(),
        files: 0,
        lines: 0,
    };
    let mut stack = vec![name.to_string()];
    parser.file(name, text, 0, &mut stack, loader);
    ParsedConfig {
        blocks: parser.blocks,
        warnings: parser.warnings,
    }
}

struct Parser {
    blocks: Vec<Block>,
    warnings: Vec<String>,
    files: usize,
    lines: usize,
}

impl Parser {
    fn file(
        &mut self,
        name: &str,
        text: &str,
        depth: usize,
        stack: &mut Vec<String>,
        loader: &mut dyn IncludeLoader,
    ) {
        self.files += 1;
        for (index, raw) in text.lines().enumerate() {
            self.lines += 1;
            if self.lines > MAX_LINES {
                if self.lines == MAX_LINES + 1 {
                    self.warnings.push(format!(
                        "the configuration is longer than {MAX_LINES} lines; the rest was ignored"
                    ));
                }
                return;
            }
            let place = format!("{name}:{}", index + 1);
            let Some((keyword, args)) = split_line(raw) else {
                continue;
            };
            match keyword.as_str() {
                "host" => self.blocks.push(Block {
                    scope: Scope::Host(args),
                    directives: Vec::new(),
                }),
                "match" => {
                    self.warnings.push(format!(
                        "{place}: a Match block was skipped (its conditions cannot be evaluated); \
                         the options inside it are not imported"
                    ));
                    self.blocks.push(Block {
                        scope: Scope::Match,
                        directives: Vec::new(),
                    });
                }
                "include" => {
                    for pattern in args {
                        self.include(&place, &pattern, depth, stack, loader);
                    }
                }
                _ => {
                    if let Some(block) = self.blocks.last_mut() {
                        block.directives.push(Directive {
                            keyword,
                            args,
                            place,
                        });
                    }
                }
            }
        }
    }

    fn include(
        &mut self,
        place: &str,
        pattern: &str,
        depth: usize,
        stack: &mut Vec<String>,
        loader: &mut dyn IncludeLoader,
    ) {
        if depth >= MAX_INCLUDE_DEPTH {
            self.warnings.push(format!(
                "{place}: Include {pattern} was not followed (nested more than {MAX_INCLUDE_DEPTH} deep)"
            ));
            return;
        }
        for file in loader.load(pattern) {
            if self.files >= MAX_FILES {
                self.warnings.push(format!(
                    "{place}: Include {pattern}: more than {MAX_FILES} files; the rest were ignored"
                ));
                return;
            }
            if stack.contains(&file.name) {
                self.warnings.push(format!(
                    "{place}: Include {pattern} includes {} again; skipped",
                    file.name
                ));
                continue;
            }
            stack.push(file.name.clone());
            self.file(&file.name, &file.contents, depth + 1, stack, loader);
            stack.pop();
        }
    }
}

/// Splits a line into its keyword (lowercase) and arguments; `None` for
/// blank lines and comments.
fn split_line(line: &str) -> Option<(String, Vec<String>)> {
    let line = line.trim_start();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let keyword_end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let keyword = line[..keyword_end].to_ascii_lowercase();
    if keyword.is_empty() {
        return None;
    }
    let mut rest = line[keyword_end..].trim_start();
    if let Some(after) = rest.strip_prefix('=') {
        rest = after.trim_start();
    }
    Some((keyword, split_args(rest)))
}

/// Whitespace-separated words; `"double quotes"` keep spaces (`\"` inside);
/// an unquoted `#` starts a comment.
fn split_args(rest: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quoted = false;
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' => quoted = false,
                '\\' if chars.peek() == Some(&'"') => {
                    current.push('"');
                    chars.next();
                }
                other => current.push(other),
            }
        } else if c == '"' {
            quoted = true;
            in_word = true;
        } else if c == '#' && !in_word {
            break;
        } else if c.is_whitespace() {
            if in_word {
                args.push(std::mem::take(&mut current));
                in_word = false;
            }
        } else {
            current.push(c);
            in_word = true;
        }
    }
    if in_word {
        args.push(current);
    }
    args
}

/// `*` and `?` wildcards, ASCII case-insensitive (host names are). Public so
/// that a loader can expand the wildcards of an `Include` with the same rule.
pub fn pattern_matches(pattern: &str, text: &str) -> bool {
    glob_match(pattern, text)
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let text: Vec<char> = text.to_ascii_lowercase().chars().collect();
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            mark = t;
            p += 1;
        } else if let Some(star_at) = star {
            p = star_at + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

/// A `Host` line applies to `name` when a positive pattern matches it and no
/// negated one does.
fn host_line_matches(patterns: &[String], name: &str) -> bool {
    let mut positive = false;
    for pattern in patterns {
        match pattern.strip_prefix('!') {
            Some(negated) => {
                if glob_match(negated, name) {
                    return false;
                }
            }
            None => positive |= glob_match(pattern, name),
        }
    }
    positive
}

fn is_plain_alias(pattern: &str) -> bool {
    !pattern.is_empty() && !pattern.starts_with('!') && !pattern.contains(['*', '?'])
}

/// A `ProxyJump` list: `[user@]host[:port]` or `ssh://[user@]host[:port]`,
/// separated by commas. `none` is an empty list.
fn parse_proxy_jump(value: &str) -> Result<Vec<JumpRef>, String> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(parse_jump_ref)
        .collect()
}

fn parse_jump_ref(entry: &str) -> Result<JumpRef, String> {
    let rest = entry.strip_prefix("ssh://").unwrap_or(entry);
    let rest = rest.trim_end_matches('/');
    let (user, hostport) = match rest.rsplit_once('@') {
        Some((user, hostport)) => (Some(user), hostport),
        None => (None, rest),
    };
    if user.is_some_and(str::is_empty) {
        return Err(format!("{entry:?} has an empty user name"));
    }
    let (host, port) = if let Some(inner) = hostport.strip_prefix('[') {
        // `[v6]` or `[v6]:port`.
        let (host, after) = inner
            .split_once(']')
            .ok_or_else(|| format!("{entry:?} has an opening '[' without a closing ']'"))?;
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port),
            None if after.is_empty() => None,
            None => return Err(format!("{entry:?} has text after the address")),
        };
        (host, port)
    } else if hostport.matches(':').count() > 1 {
        // A bare IPv6 address has no room for a port.
        (hostport, None)
    } else {
        match hostport.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (hostport, None),
        }
    };
    if host.is_empty() {
        return Err(format!("{entry:?} has no host"));
    }
    let port = match port {
        Some(text) => Some(
            text.parse::<u16>()
                .ok()
                .filter(|port| *port != 0)
                .ok_or_else(|| format!("{entry:?} has an invalid port"))?,
        ),
        None => None,
    };
    Ok(JumpRef {
        host: host.to_string(),
        user: user.map(str::to_string),
        port,
    })
}

impl ParsedConfig {
    /// The options that apply to `host` (an alias or any name), by the rules
    /// in the module docs.
    pub fn effective(&self, host: &str) -> Effective {
        let mut out = Effective::default();
        for block in &self.blocks {
            let applies = match &block.scope {
                Scope::Global => true,
                Scope::Host(patterns) => host_line_matches(patterns, host),
                Scope::Match => false,
            };
            if !applies {
                continue;
            }
            for directive in &block.directives {
                apply(&mut out, directive);
            }
        }
        out
    }

    /// The hosts this configuration declares: every plain alias of every
    /// `Host` line, once, in file order, with the options that apply.
    pub fn candidates(&self) -> Vec<ImportCandidate> {
        let mut seen: Vec<&str> = Vec::new();
        let mut out = Vec::new();
        for block in &self.blocks {
            let Scope::Host(patterns) = &block.scope else {
                continue;
            };
            for alias in patterns.iter().filter(|p| is_plain_alias(p)) {
                if seen.contains(&alias.as_str()) || !host_line_matches(patterns, alias) {
                    continue;
                }
                seen.push(alias);
                if out.len() >= MAX_CANDIDATES {
                    return out;
                }
                out.push(self.candidate(alias));
            }
        }
        out
    }

    /// What applies to `name` — a declared alias or any other name, such as a
    /// jump host that only `ProxyJump` mentions — as an import candidate.
    pub fn describe(&self, name: &str) -> ImportCandidate {
        self.candidate(name)
    }

    fn candidate(&self, alias: &str) -> ImportCandidate {
        let effective = self.effective(alias);
        let mut warnings = effective.warnings.clone();
        let host = match &effective.host_name {
            Some(name) => expand_host_tokens(name, alias, &mut warnings),
            None => alias.to_string(),
        };
        if effective.proxy_command.is_some() && effective.proxy_jump.is_none() {
            warnings.push(
                "uses ProxyCommand, which cannot be imported; the host is imported without it"
                    .to_string(),
            );
        }
        ImportCandidate {
            alias: alias.to_string(),
            host,
            user: effective.user,
            port: effective.port.unwrap_or(22),
            identity_files: effective.identity_files,
            jump_chain: effective.proxy_jump.unwrap_or_default(),
            warnings,
        }
    }
}

/// Applies one directive to `out`: the first value of a single-valued option
/// stays, `IdentityFile` accumulates.
fn apply(out: &mut Effective, directive: &Directive) {
    let first = directive.args.first().map(String::as_str);
    match directive.keyword.as_str() {
        "hostname" if out.host_name.is_none() => {
            out.host_name = first.map(str::to_string);
        }
        "user" if out.user.is_none() => out.user = first.map(str::to_string),
        "port" if out.port.is_none() => match first.map(str::parse::<u16>) {
            Some(Ok(port)) if port != 0 => out.port = Some(port),
            _ => out.warnings.push(format!(
                "{}: Port {} is not a port number; ignored",
                directive.place,
                first.unwrap_or("")
            )),
        },
        "identityfile" => {
            if let Some(path) = first
                && !path.eq_ignore_ascii_case("none")
            {
                out.identity_files.push(path.to_string());
            }
        }
        "proxyjump" if out.proxy_jump.is_none() => {
            // The value is the whole argument text, commas included.
            match parse_proxy_jump(&directive.args.join(" ")) {
                Ok(list) => out.proxy_jump = Some(list),
                Err(problem) => out
                    .warnings
                    .push(format!("{}: ProxyJump {problem}; ignored", directive.place)),
            }
        }
        "proxycommand" if out.proxy_command.is_none() => {
            let command = directive.args.join(" ");
            // `none` is how a config turns an inherited ProxyCommand off.
            if !command.eq_ignore_ascii_case("none") {
                out.proxy_command = Some(command);
            }
        }
        _ => {}
    }
}

/// `%h` is the alias, `%%` a percent sign; any other token cannot be
/// expanded here and is reported.
fn expand_host_tokens(value: &str, alias: &str, warnings: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => out.push_str(alias),
            Some('%') => out.push('%'),
            Some(other) => {
                warnings.push(format!(
                    "HostName uses the token %{other}, which cannot be expanded; it was kept as written"
                ));
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(text: &str) -> ParsedConfig {
        parse("config", text, &mut NoIncludes)
    }

    fn candidate(config: &ParsedConfig, alias: &str) -> ImportCandidate {
        config
            .candidates()
            .into_iter()
            .find(|candidate| candidate.alias == alias)
            .unwrap_or_else(|| panic!("no candidate {alias}: {:?}", config.candidates()))
    }

    // ---------------------------------------------------------- lexing --

    #[test]
    fn keywords_are_case_insensitive_and_separators_are_forgiving() {
        let config = parse_str(
            "HOST web\n  hostname=web.example.com\n  USER   deploy\n  Port = 2222\nhost\tdb\n\tHostName\tdb.example.com\n",
        );
        let web = candidate(&config, "web");
        assert_eq!(web.host, "web.example.com");
        assert_eq!(web.user.as_deref(), Some("deploy"));
        assert_eq!(web.port, 2222);
        assert_eq!(candidate(&config, "db").host, "db.example.com");
    }

    #[test]
    fn comments_blank_lines_and_quotes() {
        let config = parse_str(
            "# a comment\n\n   # an indented comment\nHost web # trailing\n  HostName \"web host.example\" # also trailing\n  IdentityFile \"~/my keys/id #1\"\n",
        );
        let web = candidate(&config, "web");
        assert_eq!(web.host, "web host.example");
        assert_eq!(web.identity_files, vec!["~/my keys/id #1"]);
        // The `#` inside quotes is part of the value; the alias is `web`.
        assert_eq!(config.candidates().len(), 1);
    }

    #[test]
    fn escaped_quotes_inside_quoted_values() {
        assert_eq!(
            split_args(r#""say \"hi\"" next"#),
            vec![r#"say "hi""#, "next"]
        );
        assert_eq!(split_args("a   b\tc"), vec!["a", "b", "c"]);
        assert_eq!(
            split_args("a#b c"),
            vec!["a#b", "c"],
            "# inside a word is not a comment"
        );
        assert!(split_args("# only a comment").is_empty());
        assert_eq!(
            split_args("\"unterminated value"),
            vec!["unterminated value"]
        );
    }

    #[test]
    fn crlf_line_endings_and_no_trailing_newline() {
        let config = parse_str("Host web\r\n  HostName web.example.com\r\n  Port 22");
        assert_eq!(candidate(&config, "web").host, "web.example.com");
    }

    // ---------------------------------------------------------- hosts ---

    #[test]
    fn every_plain_alias_of_a_host_line_is_a_candidate() {
        let config = parse_str("Host web web2 *.skip !nope ? db-?\n  User deploy\n");
        let aliases: Vec<String> = config.candidates().into_iter().map(|c| c.alias).collect();
        assert_eq!(aliases, vec!["web", "web2"]);
    }

    #[test]
    fn a_host_declared_twice_is_one_candidate() {
        let config = parse_str("Host web\n  User first\nHost web\n  User second\n  Port 2200\n");
        let candidates = config.candidates();
        assert_eq!(candidates.len(), 1);
        // First value wins; the later block still contributes what the
        // first did not set.
        assert_eq!(candidates[0].user.as_deref(), Some("first"));
        assert_eq!(candidates[0].port, 2200);
    }

    #[test]
    fn a_wildcard_only_block_is_no_host_but_gives_defaults() {
        let config = parse_str(
            "Host web\n  HostName web.example.com\n\nHost *\n  User fallback\n  Port 2022\n  IdentityFile ~/.ssh/default\n",
        );
        let candidates = config.candidates();
        assert_eq!(candidates.len(), 1, "`Host *` is not a host");
        let web = &candidates[0];
        assert_eq!(web.user.as_deref(), Some("fallback"));
        assert_eq!(web.port, 2022);
        assert_eq!(web.identity_files, vec!["~/.ssh/default"]);
    }

    #[test]
    fn the_first_value_wins_so_a_leading_wildcard_block_overrides() {
        let config =
            parse_str("Host *\n  User everyone\n\nHost web\n  User specific\n  Port 2200\n");
        let web = candidate(&config, "web");
        assert_eq!(web.user.as_deref(), Some("everyone"));
        assert_eq!(web.port, 2200, "options nobody set earlier still apply");
    }

    #[test]
    fn options_before_the_first_host_line_apply_to_every_host() {
        let config = parse_str("User global\nPort 2200\n\nHost a\n  Port 1\nHost b\n");
        assert_eq!(candidate(&config, "a").user.as_deref(), Some("global"));
        assert_eq!(
            candidate(&config, "a").port,
            2200,
            "the global value came first"
        );
        assert_eq!(candidate(&config, "b").port, 2200);
    }

    #[test]
    fn patterns_and_negation_decide_which_blocks_apply() {
        let config = parse_str(
            "Host web1 web2 db\n  User plain\nHost web*  !web2\n  Port 2200\nHost *.corp\n  User corp\n",
        );
        assert_eq!(candidate(&config, "web1").port, 2200);
        assert_eq!(candidate(&config, "web2").port, 22, "negated by !web2");
        assert_eq!(candidate(&config, "db").port, 22);
        // A name only a pattern covers can be asked for directly.
        let effective = config.effective("anything.corp");
        assert_eq!(effective.user.as_deref(), Some("corp"));
        assert_eq!(
            config.effective("WEB1").port,
            Some(2200),
            "case-insensitive"
        );
    }

    #[test]
    fn glob_matching_handles_stars_and_question_marks() {
        for (pattern, text, expected) in [
            ("*", "anything", true),
            ("*", "", true),
            ("web?", "web1", true),
            ("web?", "web", false),
            ("web?", "web12", false),
            ("*.example.com", "a.b.example.com", true),
            ("*.example.com", "example.com", false),
            ("a*b*c", "a--b--c", true),
            ("a*b*c", "a--c--b", false),
            ("a*", "a", true),
            ("**", "x", true),
            ("", "", true),
            ("", "x", false),
        ] {
            assert_eq!(glob_match(pattern, text), expected, "{pattern} vs {text}");
        }
    }

    #[test]
    fn hostname_defaults_to_the_alias_and_expands_percent_h() {
        let config = parse_str(
            "Host plain\nHost tokens\n  HostName %h.internal.example\nHost odd\n  HostName host-%p\nHost pct\n  HostName 100%%\n",
        );
        assert_eq!(candidate(&config, "plain").host, "plain");
        assert_eq!(candidate(&config, "tokens").host, "tokens.internal.example");
        let odd = candidate(&config, "odd");
        assert_eq!(odd.host, "host-%p");
        assert!(
            odd.warnings.iter().any(|w| w.contains("%p")),
            "{:?}",
            odd.warnings
        );
        assert_eq!(candidate(&config, "pct").host, "100%");
    }

    #[test]
    fn a_bad_port_is_reported_and_the_default_stays() {
        let config = parse_str("Host web\n  Port eighty\nHost b\n  Port 0\nHost c\n  Port 70000\n");
        for alias in ["web", "b", "c"] {
            let candidate = candidate(&config, alias);
            assert_eq!(candidate.port, 22, "{alias}");
            assert!(
                candidate
                    .warnings
                    .iter()
                    .any(|w| w.contains("not a port number")),
                "{alias}: {:?}",
                candidate.warnings
            );
        }
    }

    #[test]
    fn identity_files_accumulate_in_order_and_none_is_ignored() {
        let config = parse_str(
            "Host web\n  IdentityFile ~/.ssh/a\n  IdentityFile none\n  IdentityFile ~/.ssh/b\nHost *\n  IdentityFile ~/.ssh/c\n",
        );
        assert_eq!(
            candidate(&config, "web").identity_files,
            vec!["~/.ssh/a", "~/.ssh/b", "~/.ssh/c"]
        );
    }

    // ------------------------------------------------------ ProxyJump ---

    #[test]
    fn proxy_jump_forms() {
        let config = parse_str(
            "Host a\n  ProxyJump bastion\nHost b\n  ProxyJump deploy@gw.example.com:2222,inner\nHost c\n  ProxyJump ssh://ops@jump.example.com:2200\nHost d\n  ProxyJump [fe80::1%eth0]:22,[::1]\nHost e\n  ProxyJump fe80::5\n",
        );
        assert_eq!(
            candidate(&config, "a").jump_chain,
            vec![JumpRef {
                host: "bastion".into(),
                user: None,
                port: None
            }]
        );
        assert_eq!(
            candidate(&config, "b").jump_chain,
            vec![
                JumpRef {
                    host: "gw.example.com".into(),
                    user: Some("deploy".into()),
                    port: Some(2222)
                },
                JumpRef {
                    host: "inner".into(),
                    user: None,
                    port: None
                }
            ]
        );
        assert_eq!(
            candidate(&config, "c").jump_chain,
            vec![JumpRef {
                host: "jump.example.com".into(),
                user: Some("ops".into()),
                port: Some(2200)
            }]
        );
        assert_eq!(
            candidate(&config, "d").jump_chain,
            vec![
                JumpRef {
                    host: "fe80::1%eth0".into(),
                    user: None,
                    port: Some(22)
                },
                JumpRef {
                    host: "::1".into(),
                    user: None,
                    port: None
                }
            ]
        );
        assert_eq!(
            candidate(&config, "e").jump_chain,
            vec![JumpRef {
                host: "fe80::5".into(),
                user: None,
                port: None
            }],
            "a bare IPv6 address has no port"
        );
    }

    #[test]
    fn proxy_jump_none_cancels_an_inherited_jump_and_first_value_wins() {
        let config = parse_str(
            "Host direct\n  ProxyJump none\nHost web\n  ProxyJump first\nHost *\n  ProxyJump default\n",
        );
        assert!(candidate(&config, "direct").jump_chain.is_empty());
        assert_eq!(candidate(&config, "web").jump_chain[0].host, "first");
        assert_eq!(
            config.effective("other").proxy_jump.unwrap()[0].host,
            "default"
        );
    }

    #[test]
    fn a_broken_proxy_jump_is_reported_not_fatal() {
        let config = parse_str(
            "Host a\n  ProxyJump @host\nHost b\n  ProxyJump host:99999\nHost c\n  ProxyJump [::1\nHost d\n  ProxyJump [::1]x\n",
        );
        for alias in ["a", "b", "c", "d"] {
            let candidate = candidate(&config, alias);
            assert!(candidate.jump_chain.is_empty(), "{alias}");
            assert!(
                candidate.warnings.iter().any(|w| w.contains("ProxyJump")),
                "{alias}: {:?}",
                candidate.warnings
            );
        }
    }

    #[test]
    fn proxy_command_is_reported_unless_a_proxy_jump_replaces_it() {
        let config = parse_str(
            "Host a\n  ProxyCommand ssh -W %h:%p gateway\nHost b\n  ProxyCommand none\nHost c\n  ProxyCommand nc %h %p\n  ProxyJump gw\n",
        );
        assert!(
            candidate(&config, "a")
                .warnings
                .iter()
                .any(|w| w.contains("ProxyCommand")),
        );
        assert!(candidate(&config, "b").warnings.is_empty());
        assert!(candidate(&config, "c").warnings.is_empty());
    }

    // ----------------------------------------------------------- Match ---

    #[test]
    fn match_blocks_are_skipped_with_a_warning_that_names_the_place() {
        let config = parse_str(
            "Host web\n  User deploy\n\nMatch host web exec \"true\"\n  User intruder\n  Port 9\n\nHost db\n  Port 5\n",
        );
        assert_eq!(candidate(&config, "web").user.as_deref(), Some("deploy"));
        assert_eq!(candidate(&config, "web").port, 22);
        assert_eq!(
            candidate(&config, "db").port,
            5,
            "parsing resumes after the Match block"
        );
        assert_eq!(config.warnings.len(), 1);
        assert!(
            config.warnings[0].contains("config:4"),
            "{:?}",
            config.warnings
        );
        assert!(
            config.warnings[0].contains("Match"),
            "{:?}",
            config.warnings
        );
    }

    // ---------------------------------------------------------- Include --

    struct Files(Vec<(&'static str, &'static str, &'static str)>);

    impl IncludeLoader for Files {
        fn load(&mut self, pattern: &str) -> Vec<LoadedFile> {
            self.0
                .iter()
                .filter(|(wanted, _, _)| *wanted == pattern)
                .map(|(_, name, contents)| LoadedFile {
                    name: (*name).to_string(),
                    contents: (*contents).to_string(),
                })
                .collect()
        }
    }

    #[test]
    fn includes_are_spliced_in_place() {
        let mut loader = Files(vec![
            (
                "conf.d/*",
                "conf.d/a",
                "Host from-a\n  HostName a.example.com\n",
            ),
            ("conf.d/*", "conf.d/b", "Host from-b\n  Port 2200\n"),
        ]);
        let config = parse(
            "config",
            "Host first\nInclude conf.d/*\nHost last\n",
            &mut loader,
        );
        let aliases: Vec<String> = config.candidates().into_iter().map(|c| c.alias).collect();
        assert_eq!(aliases, vec!["first", "from-a", "from-b", "last"]);
        assert_eq!(candidate(&config, "from-a").host, "a.example.com");
        assert_eq!(candidate(&config, "from-b").port, 2200);
    }

    #[test]
    fn an_include_inside_a_host_block_continues_that_block() {
        let mut loader = Files(vec![(
            "extra",
            "extra",
            "  User from-include\n  Port 2200\n",
        )]);
        let config = parse(
            "config",
            "Host web\n  Include extra\n  HostName w\n",
            &mut loader,
        );
        let web = candidate(&config, "web");
        assert_eq!(web.user.as_deref(), Some("from-include"));
        assert_eq!(web.port, 2200);
        assert_eq!(web.host, "w");
    }

    #[test]
    fn a_file_that_includes_itself_is_followed_once() {
        let mut loader = Files(vec![("loop", "loop", "Host in-loop\nInclude loop\n")]);
        let config = parse("config", "Include loop\n", &mut loader);
        assert_eq!(config.candidates().len(), 1);
        assert!(
            config.warnings.iter().any(|w| w.contains("again")),
            "{:?}",
            config.warnings
        );
    }

    #[test]
    fn a_cycle_between_two_files_stops() {
        let mut loader = Files(vec![
            ("a", "a", "Host from-a\nInclude b\n"),
            ("b", "b", "Host from-b\nInclude a\n"),
        ]);
        let config = parse("config", "Include a\n", &mut loader);
        let aliases: Vec<String> = config.candidates().into_iter().map(|c| c.alias).collect();
        assert_eq!(aliases, vec!["from-a", "from-b"]);
        assert!(config.warnings.iter().any(|w| w.contains("again")));
    }

    /// Distinct files that keep including a new one, forever: the depth limit
    /// ends it.
    #[test]
    fn nesting_is_limited() {
        struct Endless;
        impl IncludeLoader for Endless {
            fn load(&mut self, pattern: &str) -> Vec<LoadedFile> {
                let next: u32 = pattern.parse().unwrap_or(0) + 1;
                vec![LoadedFile {
                    name: format!("file-{next}"),
                    contents: format!("Host level-{next}\nInclude {next}\n"),
                }]
            }
        }
        let config = parse("config", "Include 0\n", &mut Endless);
        assert_eq!(config.candidates().len(), MAX_INCLUDE_DEPTH);
        assert!(
            config
                .warnings
                .iter()
                .any(|w| w.contains("nested more than"))
        );
    }

    #[test]
    fn the_number_of_included_files_is_limited() {
        struct Many;
        impl IncludeLoader for Many {
            fn load(&mut self, _pattern: &str) -> Vec<LoadedFile> {
                (0..MAX_FILES * 2)
                    .map(|n| LoadedFile {
                        name: format!("f{n}"),
                        contents: format!("Host h{n}\n"),
                    })
                    .collect()
            }
        }
        let config = parse("config", "Include *\n", &mut Many);
        assert_eq!(config.candidates().len(), MAX_FILES - 1);
        assert!(config.warnings.iter().any(|w| w.contains("more than")));
    }

    #[test]
    fn an_include_that_matches_nothing_is_fine() {
        let config = parse_str("Include missing/*\nHost web\n");
        assert_eq!(config.candidates().len(), 1);
        assert!(config.warnings.is_empty());
    }

    // ----------------------------------------------------------- limits --

    #[test]
    fn an_absurdly_long_config_is_cut_off_with_a_warning() {
        let text = "Host h\n".repeat(MAX_LINES + 50);
        let config = parse_str(&text);
        assert!(config.warnings.iter().any(|w| w.contains("longer than")));
        assert_eq!(config.candidates().len(), 1);
    }

    #[test]
    fn candidates_are_capped() {
        let text: String = (0..MAX_CANDIDATES + 10)
            .map(|n| format!("Host h{n}\n"))
            .collect();
        assert_eq!(parse_str(&text).candidates().len(), MAX_CANDIDATES);
    }

    #[test]
    fn garbage_does_not_panic() {
        for text in [
            "",
            "\0\0\0",
            "=",
            "Host",
            "Host =",
            "HostName",
            "ProxyJump",
            "Include",
            "\"\"\"\"",
            "Host \"\"\n  User x",
            "\u{feff}Host web\n",
            "Host web\nPort",
        ] {
            let config = parse_str(text);
            let _ = config.candidates();
        }
    }

    #[test]
    fn a_realistic_config() {
        let text = "\
# Work
Host bastion
  HostName bastion.corp.example
  User ops
  IdentityFile ~/.ssh/work_ed25519
  Port 2222

Host app-*
  ProxyJump bastion
  User deploy

Host app-1 app-2
  HostName %h.corp.example

Host github.com
  User git
  IdentityFile ~/.ssh/github

Match user root
  ForwardAgent yes

Host *
  ServerAliveInterval 30
  IdentityFile ~/.ssh/id_ed25519
";
        let config = parse_str(text);
        let aliases: Vec<String> = config.candidates().into_iter().map(|c| c.alias).collect();
        assert_eq!(aliases, vec!["bastion", "app-1", "app-2", "github.com"]);
        let app = candidate(&config, "app-1");
        assert_eq!(app.host, "app-1.corp.example");
        assert_eq!(app.user.as_deref(), Some("deploy"));
        assert_eq!(app.jump_chain.len(), 1);
        assert_eq!(app.jump_chain[0].host, "bastion");
        assert_eq!(app.identity_files, vec!["~/.ssh/id_ed25519"]);
        let bastion = candidate(&config, "bastion");
        assert_eq!(bastion.port, 2222);
        assert_eq!(
            bastion.identity_files,
            vec!["~/.ssh/work_ed25519", "~/.ssh/id_ed25519"]
        );
        assert_eq!(config.warnings.len(), 1, "{:?}", config.warnings);
    }
}
