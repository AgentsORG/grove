//! `grove`: source control for people and agents, with one graph underneath.

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use grove_core::*;
use std::path::PathBuf;

mod http;

#[derive(Parser)]
#[command(
    name = "grove",
    version,
    about = "Source control for people and agents, with one graph underneath."
)]
struct Cli {
    /// Act as someone: `agent:codex`, `person:meera`, or a name. Also GROVE_AS.
    #[arg(long = "as", global = true, value_name = "WHO")]
    as_: Option<String>,
    /// The person an agent is working for. Also GROVE_FOR.
    #[arg(long = "for", global = true, value_name = "PERSON")]
    for_: Option<String>,
    /// Print JSON instead of text, for agents and scripts.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start a repository here, recording what's already in the folder.
    Init {
        /// A check to run before anything lands: NAME=COMMAND. Repeatable.
        #[arg(long = "check", value_name = "NAME=COMMAND")]
        checks: Vec<String>,
        /// Add `zero check` as a check (needs the zero compiler on PATH).
        #[arg(long)]
        zero: bool,
    },
    /// Add or list workspaces: separate folders for people and agents.
    #[command(subcommand)]
    Workspace(WorkspaceCmd),
    /// What this workspace has changed since it last synced.
    Status,
    /// Record this workspace's edits as a change (a new version if one is in progress).
    New {
        /// Why: one line of intent.
        intent: String,
    },
    /// Land a change on main. Defaults to this workspace's change.
    Land { change: Option<String> },
    /// Changes recorded but not landed.
    Drafts,
    /// Main's history.
    Log {
        #[arg(short = 'n', default_value_t = 20)]
        n: usize,
    },
    /// One change: who, why, what it touches, its ops.
    Show { change: String },
    /// Who last changed a declaration, and why.
    Why {
        name: String,
        /// Every change that touched it, including renames.
        #[arg(long)]
        all: bool,
    },
    /// The program graph on main: declarations and their edges.
    Graph {
        /// Graphviz output.
        #[arg(long)]
        dot: bool,
    },
    /// Bring this workspace up to main, keeping your unrecorded edits.
    Sync,
    /// Print a file as it is on main.
    Cat { file: String },
    /// Undo the last operation (land, record, sync, ...).
    Undo,
    /// The operation log.
    Ops {
        #[arg(short = 'n', default_value_t = 20)]
        n: usize,
    },
    /// Open conflicts, or one conflict in full.
    Conflicts { id: Option<String> },
    /// Close a conflict once it is resolved.
    Resolve { id: String },
    /// Check that every stored object still matches its address.
    Verify,
    /// Named remotes: a GroveHub repository URL or another repository's folder.
    #[command(subcommand)]
    Remote(RemoteCmd),
    /// Copy a remote repository into a new folder, with a workspace for you.
    Clone { url: String, dir: Option<PathBuf> },
    /// Get what's new on a remote, without moving main.
    Fetch {
        #[arg(default_value = "origin")]
        remote: String,
    },
    /// Fetch, move main to the remote's, and sync this workspace.
    Pull {
        #[arg(default_value = "origin")]
        remote: String,
    },
    /// Send work to a remote: landings it lacks, then this workspace's
    /// change, which the remote replays, checks and lands.
    Push {
        #[arg(default_value = "origin")]
        remote: String,
        /// Push this change instead of the workspace's.
        #[arg(long)]
        change: Option<String>,
        /// Keep the change open on the remote for review instead of landing it.
        #[arg(long)]
        review: bool,
    },
    /// A git-style text diff: this workspace's unrecorded edits, or a change.
    Diff { change: Option<String> },
    /// Undo what a landed change did, as a new change.
    Revert { change: String },
    /// Apply a change from anywhere (a lane, an undone landing) here.
    Pick { change: String },
    /// Throw away unrecorded edits: to these files, or all of them.
    Restore { paths: Vec<String> },
    /// Put unrecorded edits aside under a name.
    Shelve {
        #[arg(default_value = "wip")]
        name: String,
    },
    /// Bring shelved edits back.
    Unshelve {
        #[arg(default_value = "wip")]
        name: String,
    },
    /// Shelved edits.
    Shelves,
    /// Name a landing (a release). Without a name, list tags.
    Tag {
        name: Option<String>,
        /// The landing, change or tag to name (default: main).
        #[arg(long)]
        at: Option<String>,
        /// Remove the tag instead.
        #[arg(long)]
        delete: bool,
    },
    /// Lanes: named lines of work beside main (git's branches).
    #[command(subcommand)]
    Lane(LaneCmd),
    /// Lines on main containing some text.
    Find { query: String },
    /// Every version of a change.
    Evolve { change: String },
    /// Landed changes per person and agent.
    Shortlog,
    /// Find the landing where a command started failing.
    Bisect {
        /// The test: exits 0 when good.
        #[arg(long)]
        run: String,
        /// A landing, change or tag where it passed (default: the first).
        #[arg(long)]
        good: Option<String>,
    },
    /// Your signing key: make one, show it, or trust it in this repository.
    #[command(subcommand)]
    Key(KeyCmd),
    /// Approve a draft (people only), for the review rules.
    Approve { change: String },
    /// Add a note to a change.
    Note { change: String, text: String },
    /// Who is working on what, in every workspace.
    Who,
    /// Review rules: `grove config review.signature true`. No value prints it.
    Config {
        key: Option<String>,
        value: Option<String>,
    },
    /// Work with git: export to a git repository, push to GitHub or GitLab,
    /// import a git repository's history.
    #[command(subcommand)]
    Git(GitCmd),
}

#[derive(Subcommand)]
enum KeyCmd {
    /// Make a signing key for whoever is acting (or show the one they have).
    New,
    /// Print your public key.
    Show,
    /// Trust your key in this repository, so your changes count as signed.
    Trust,
}

#[derive(Subcommand)]
enum LaneCmd {
    /// Start a lane from main (or from a tag or landing).
    New {
        name: String,
        #[arg(long)]
        from: Option<String>,
    },
    /// Point this workspace at a lane; `main` goes back to main.
    Use { name: String },
    /// Land a lane's work on main as one change.
    Land { name: String },
    /// Lanes and their heads.
    List,
    /// Delete a lane.
    Drop { name: String },
}

#[derive(Subcommand)]
enum GitCmd {
    /// Write main's history as git commits in `.grove/git` (one per landing).
    Export,
    /// Export, then push main to a git remote URL.
    Push {
        remote: String,
        /// Overwrite the remote's main if it moved (uses --force-with-lease).
        #[arg(long)]
        force: bool,
    },
    /// Bring in commits made on a git remote's main (merged when Grove moved too).
    Pull { remote: String },
    /// Open a pull request on a git host from this workspace's change (or another).
    Pr {
        remote: String,
        #[arg(long)]
        change: Option<String>,
    },
    /// Make a new Grove repository from a git repository's history.
    Import {
        source: PathBuf,
        dest: PathBuf,
        /// Only the last N commits of the first-parent history.
        #[arg(long)]
        last: Option<usize>,
    },
}

#[derive(Subcommand)]
enum RemoteCmd {
    /// Name a remote: `grove remote add origin http://localhost:4600/you/repo`.
    Add {
        name: String,
        url: String,
    },
    List,
}

#[derive(Subcommand)]
enum WorkspaceCmd {
    /// A new workspace in a new folder, holding main.
    Add {
        name: String,
        path: Option<PathBuf>,
        /// Hold only these paths (repeatable): a partial workspace.
        #[arg(long = "only", value_name = "PATH")]
        only: Vec<String>,
    },
    List,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("grove: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;
    let flag_actor = || -> Option<Actor> {
        let who = cli.as_.clone().or_else(|| std::env::var("GROVE_AS").ok())?;
        let for_ = cli.for_.clone().or_else(|| std::env::var("GROVE_FOR").ok());
        Some(Actor::parse(&who, for_.as_deref()))
    };
    let default_person = || {
        Actor::person(
            &std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "someone".into()),
        )
    };

    if let Cmd::Init { checks, zero } = &cli.cmd {
        let actor = flag_actor().unwrap_or_else(default_person);
        let mut specs = vec![];
        for c in checks {
            let Some((name, run)) = c.split_once('=') else {
                bail!("--check wants NAME=COMMAND, got {c}")
            };
            specs.push(CheckSpec {
                name: name.into(),
                run: run.into(),
            });
        }
        if *zero {
            // Grove never tracks zero.graph (its ids are machine-local), so the
            // check rebuilds it from source in the scratch copy first.
            specs.push(CheckSpec {
                name: "zero check".into(),
                run: "zero import . && zero check .".into(),
            });
        }
        let repo = Repo::init(&cwd, &actor, specs)?;
        let view = repo.main_view()?;
        println!("Started a Grove repository in {}", repo.root.display());
        println!(
            "  {} declarations, {} edges, as {actor}",
            view.decls.len(),
            view.edges().len()
        );
        return Ok(());
    }

    if let Cmd::Git(GitCmd::Import { source, dest, last }) = &cli.cmd {
        let actor = flag_actor().unwrap_or_else(default_person);
        let (repo, n) = grove_core::git::import(source, dest, &actor, *last)?;
        let view = repo.main_view()?;
        println!("Imported {n} commit(s) into {}", repo.root.display());
        println!(
            "  {} declarations, {} edges",
            view.decls.len(),
            view.edges().len()
        );
        return Ok(());
    }

    if let Cmd::Clone { url, dir } = &cli.cmd {
        let actor = flag_actor().unwrap_or_else(default_person);
        let dest = dir.clone().unwrap_or_else(|| {
            cwd.join(
                url.trim_end_matches(['/', '\\'])
                    .trim_end_matches(".git")
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("repo"),
            )
        });
        let remote = http::open(url)?;
        let repo = Repo::clone_from(url, remote.as_ref(), &dest, &actor)?;
        let view = repo.main_view()?;
        println!("Cloned {url} into {}", repo.root.display());
        println!(
            "  {} declarations, {} edges, working as {actor}",
            view.decls.len(),
            view.edges().len()
        );
        return Ok(());
    }

    let (repo, ws) = Repo::discover(&cwd)?;
    let refs = repo.refs()?;
    let actor = flag_actor()
        .or_else(|| refs.workspaces.get(&ws).and_then(|w| w.actor.clone()))
        .unwrap_or_else(default_person);

    match cli.cmd {
        Cmd::Init { .. } => unreachable!(),
        Cmd::Workspace(WorkspaceCmd::Add { name, path, only }) => {
            let path = path.unwrap_or_else(|| {
                repo.root.parent().unwrap_or(&repo.root).join(format!(
                    "{}-{name}",
                    repo.root
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                ))
            });
            let who = flag_actor().unwrap_or_else(|| actor.clone());
            let p = repo.add_workspace_only(&name, &path, &who, &only)?;
            println!("Workspace {name} for {who} at {}", p.display());
            if !only.is_empty() {
                println!("  holding only {}", only.join(", "));
            }
        }
        Cmd::Workspace(WorkspaceCmd::List) => {
            for (name, w) in &refs.workspaces {
                let here = if *name == ws { "*" } else { " " };
                let who = w.actor.as_ref().map(|a| a.to_string()).unwrap_or_default();
                let draft = w
                    .draft
                    .as_deref()
                    .map(|d| format!("  draft {}", short(d)))
                    .unwrap_or_default();
                println!("{here} {name:<12} {who:<24} {}{draft}", w.path);
            }
        }
        Cmd::Status => {
            let ops = repo.status(&ws)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&ops)?);
            } else if ops.is_empty() {
                println!("Nothing changed in {ws} since it last synced.");
            } else {
                println!("{} change(s) in {ws}:", ops.len());
                let base = repo.view_at(&refs.workspaces[&ws].base)?;
                for op in &ops {
                    println!("  {}", describe(op, &base));
                }
            }
        }
        Cmd::New { intent } => {
            let c = repo.record(&ws, &intent, &actor)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&c)?);
            } else {
                let view = repo.view_at(&c.base)?;
                println!(
                    "Recorded {} v{} \"{}\" as {}",
                    short(&c.id),
                    c.version,
                    c.intent,
                    c.actor
                );
                println!("  writes   {}", names(&c.footprint.writes, &view, &c.ops));
                if !c.footprint.rewrites.is_empty() {
                    println!("  rewrites {}", names(&c.footprint.rewrites, &view, &c.ops));
                }
                if !c.footprint.reads.is_empty() {
                    println!("  reads    {}", names(&c.footprint.reads, &view, &c.ops));
                }
            }
        }
        Cmd::Land { change } => {
            match repo.land(&ws, change.as_deref(), &actor)? {
                LandOutcome::Landed {
                    change,
                    mode,
                    evidence,
                    synced,
                    ..
                } => {
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::json!({"landed": change.id, "mode": mode, "evidence": evidence})
                        );
                    } else {
                        println!(
                            "Landed {} \"{}\" ({})",
                            short(&change.id),
                            change.intent,
                            mode_word(&mode)
                        );
                        for e in &evidence {
                            println!(
                                "  {} {}{}",
                                if e.ok { "ok  " } else { "FAIL" },
                                e.check,
                                if e.detail.is_empty() {
                                    String::new()
                                } else {
                                    format!(": {}", e.detail.lines().next().unwrap_or(""))
                                }
                            );
                        }
                        if !synced.is_empty() {
                            println!("  synced {} to the new main", synced.join(", "));
                        }
                    }
                }
                LandOutcome::Rejected { change, evidence } => {
                    println!(
                        "Did not land {} \"{}\": {}",
                        short(&change.id),
                        change.intent,
                        refusal(&change, &evidence)
                    );
                    for e in evidence.iter().filter(|e| !e.ok) {
                        println!("  FAIL {}\n{}", e.check, indent(&e.detail, "       "));
                    }
                    std::process::exit(2);
                }
                LandOutcome::Conflict {
                    change,
                    id,
                    conflict,
                } => {
                    println!("Did not land {} \"{}\": it sets the same declaration as work already on main.", short(&change.id), change.intent);
                    for d in &conflict.decls {
                        println!(
                            "  {} (also changed by {})",
                            d.name,
                            conflict
                                .against
                                .iter()
                                .map(|c| short(c))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    println!("Saved as conflict {}. See it with `grove conflicts {}`; anyone can take it.", short(&id), short(&id));
                    std::process::exit(3);
                }
            }
        }
        Cmd::Drafts => {
            for (id, h) in &refs.drafts {
                let c = repo.change(h)?;
                println!(
                    "{}  v{}  {:<28} {}",
                    short(id),
                    c.version,
                    c.actor.to_string(),
                    c.intent
                );
            }
        }
        Cmd::Log { n } => {
            for (h, l) in repo.history(&repo.main()?, None)?.into_iter().take(n) {
                match &l.change {
                    Some(ch) => {
                        let c = repo.change(ch)?;
                        println!(
                            "{}  {}  {:<28} {}  ({})",
                            h.short(),
                            short(&c.id),
                            c.actor.to_string(),
                            c.intent,
                            mode_word(&l.mode)
                        );
                    }
                    None => println!("{}  started the repository", h.short()),
                }
            }
        }
        Cmd::Show { change } => {
            let c = repo.find_change(&change)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&c)?);
            } else {
                let view = repo.view_at(&c.base)?;
                println!("{} v{}  \"{}\"", short(&c.id), c.version, c.intent);
                println!("  by       {}", c.actor);
                println!("  writes   {}", names(&c.footprint.writes, &view, &c.ops));
                println!("  rewrites {}", names(&c.footprint.rewrites, &view, &c.ops));
                println!("  reads    {}", names(&c.footprint.reads, &view, &c.ops));
                println!("  ops");
                for op in &c.ops {
                    println!("    {}", describe(op, &view));
                }
                let config = repo.config()?;
                let sig = match grove_core::trust::verify(&c, &config.keys) {
                    grove_core::trust::SigStatus::Unsigned => "unsigned".to_string(),
                    grove_core::trust::SigStatus::Invalid => {
                        "INVALID: changed after signing".to_string()
                    }
                    grove_core::trust::SigStatus::Valid { trusted, key } => format!(
                        "{} key {}",
                        if trusted { "trusted" } else { "untrusted" },
                        &key[..12]
                    ),
                };
                println!("  signed   {sig}");
                if let Some(list) = refs.approvals.get(&c.id) {
                    for a in list {
                        println!("  approved by {} (v{})", a.by.name, a.version);
                    }
                }
                for n in repo.notes(&c.id)? {
                    println!("  note     {}: {}", n.by, n.text);
                }
            }
        }
        Cmd::Why { name, all } => {
            let (_, hits) = repo.why(&name, all)?;
            if hits.is_empty() {
                println!("{name} hasn't changed since the repository started.");
            }
            for (l, c) in hits {
                println!(
                    "{}  {}  \"{}\"  ({})",
                    short(&c.id),
                    c.actor,
                    c.intent,
                    mode_word(&l.mode)
                );
            }
        }
        Cmd::Graph { dot } => {
            let view = repo.main_view()?;
            let edges = view.edges();
            if dot {
                println!("digraph grove {{");
                for (id, n) in &view.decls {
                    println!(
                        "  \"{id}\" [label=\"{}\"{}];",
                        n.entry.name.replace('"', "'"),
                        if n.is_code() { "" } else { " shape=note" }
                    );
                }
                for e in &edges {
                    println!(
                        "  \"{}\" -> \"{}\"{};",
                        e.from,
                        e.to,
                        if e.kind == EdgeKind::Mention {
                            " [style=dashed]"
                        } else {
                            ""
                        }
                    );
                }
                println!("}}");
            } else if cli.json {
                let nodes: Vec<_> = view.decls.iter().map(|(id, n)| serde_json::json!({"id": id, "name": n.entry.name, "kind": n.entry.kind, "file": n.entry.file})).collect();
                let es: Vec<_> = edges.iter().map(|e| serde_json::json!({"from": e.from, "to": e.to, "kind": format!("{:?}", e.kind).to_lowercase()})).collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"nodes": nodes, "edges": es, "edgesFrom": grove_core::zero::edge_source(&view)})
                    )?
                );
            } else {
                println!(
                    "{} declarations, {} edges (from {})",
                    view.decls.len(),
                    edges.len(),
                    grove_core::zero::edge_source(&view)
                );
                for (id, n) in &view.decls {
                    let out: Vec<_> = edges
                        .iter()
                        .filter(|e| &e.from == id)
                        .map(|e| {
                            format!(
                                "{}{}",
                                view.decls[&e.to].entry.name,
                                if e.kind == EdgeKind::Mention { "?" } else { "" }
                            )
                        })
                        .collect();
                    if !out.is_empty() {
                        println!("  {} -> {}", n.entry.name, out.join(", "));
                    }
                }
                println!("  (? marks an inferred edge from a non-code file)");
            }
        }
        Cmd::Sync => println!("{}", repo.sync(&ws, &actor)?),
        Cmd::Cat { file } => {
            let files = repo.main_view()?.files();
            let key = file.replace('\\', "/");
            match files.get(&key) {
                Some(text) => print!("{text}"),
                None => bail!("{file} is not on main"),
            }
        }
        Cmd::Undo => println!("{}", repo.undo(&actor)?),
        Cmd::Ops { n } => {
            let log = repo.oplog()?;
            for e in log.iter().rev().take(n) {
                println!("{:>4}  {:<9} {:<24} {}", e.seq, e.kind, e.actor, e.summary);
            }
        }
        Cmd::Conflicts { id } => match id {
            None => {
                if refs.conflicts.is_empty() {
                    println!("No open conflicts.");
                }
                for cid in refs.conflicts.keys() {
                    let (_, c) = repo.conflict(cid)?;
                    println!(
                        "{}  change {}  on {}",
                        short(cid),
                        short(&c.change),
                        c.decls
                            .iter()
                            .map(|d| d.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            Some(id) => {
                let (cid, c) = repo.conflict(&id)?;
                println!(
                    "Conflict {}: change {} against {}",
                    short(&cid),
                    short(&c.change),
                    c.against
                        .iter()
                        .map(|i| short(i))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                for d in &c.decls {
                    println!("\n{}", d.name);
                    for (label, src) in [
                        ("before", &d.base),
                        ("on main", &d.main),
                        ("this change", &d.mine),
                    ] {
                        println!("  -- {label}");
                        println!(
                            "{}",
                            indent(src.as_deref().unwrap_or("(absent)\n"), "     ")
                        );
                    }
                }
                println!("\nTo resolve: sync your workspace, edit {}, `grove new` and `grove land`, then `grove resolve {}`.", c.decls.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(", "), short(&cid));
            }
        },
        Cmd::Resolve { id } => println!("{}", repo.close_conflict(&id, &actor)?),
        Cmd::Git(GitCmd::Export) => {
            let (mirror, n) = grove_core::git::export(&repo)?;
            println!("Wrote {n} new commit(s); git main matches Grove main.");
            println!("  git repository: {}", mirror.dir.display());
            println!("  push it anywhere: grove git push <remote-url>");
        }
        Cmd::Git(GitCmd::Push { remote, force }) => {
            println!("{}", grove_core::git::push(&repo, &remote, force)?)
        }
        Cmd::Git(GitCmd::Import { .. }) => unreachable!(),
        Cmd::Clone { .. } => unreachable!(),
        Cmd::Remote(RemoteCmd::Add { name, url }) => {
            if url.starts_with("http://") || url.starts_with("https://") {
                http::Http::new(&url)?;
            }
            repo.set_remote(&name, &url)?;
            println!("Remote {name} is {url}");
        }
        Cmd::Remote(RemoteCmd::List) => {
            for (name, url) in repo.config()?.remotes {
                let at = refs
                    .remotes
                    .get(&name)
                    .map(|h| format!("  main at {}", h.short()))
                    .unwrap_or_default();
                println!("{name:<10} {url}{at}");
            }
        }
        Cmd::Fetch { remote } => {
            let r = http::open(&repo.remote_url(&remote)?)?;
            let (main, new) = repo.fetch_from(&remote, r.as_ref())?;
            match main {
                Some(m) => println!(
                    "Fetched {new} new object(s); {remote} main is at {}",
                    m.short()
                ),
                None => println!("{remote} is empty."),
            }
        }
        Cmd::Pull { remote } => {
            let r = http::open(&repo.remote_url(&remote)?)?;
            println!("{}", repo.pull(&ws, &remote, r.as_ref(), &actor)?);
        }
        Cmd::Push {
            remote,
            change,
            review,
        } => {
            let url = repo.remote_url(&remote)?;
            let r = http::open(&url)?;
            let report =
                repo.push_to(&ws, &remote, r.as_ref(), change.as_deref(), review, &actor)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
                return Ok(());
            }
            if report.landings > 0 {
                println!(
                    "Sent {} landing(s); {remote} main matches yours.",
                    report.landings
                );
            }
            match &report.proposal {
                None if report.landings == 0 => println!(
                    "Nothing to push: {remote} has main, and this workspace has no change recorded (`grove new` first)."
                ),
                None => {}
                Some(Proposal::Landed {
                    change,
                    intent,
                    mode,
                    evidence,
                    ..
                }) => {
                    println!(
                        "Landed {} \"{intent}\" on {remote} ({})",
                        short(change),
                        mode_word(mode)
                    );
                    for e in evidence {
                        println!(
                            "  {} {}{}",
                            if e.ok { "ok  " } else { "FAIL" },
                            e.check,
                            if e.detail.is_empty() {
                                String::new()
                            } else {
                                format!(": {}", e.detail.lines().next().unwrap_or(""))
                            }
                        );
                    }
                    if !report.synced.is_empty() {
                        println!("  synced {} to the new main", report.synced.join(", "));
                    }
                    println!("  {url}");
                }
                Some(Proposal::Open { change, version }) => {
                    println!(
                        "Sent {} v{version} to {remote} for review; it lands when someone approves it.",
                        short(change)
                    );
                    println!("  {url}/changes/{change}");
                }
                Some(Proposal::Conflict {
                    change,
                    conflict,
                    decls,
                    ..
                }) => {
                    println!(
                        "Did not land {}: it sets {} differently from work already on {remote}.",
                        short(change),
                        decls.join(", ")
                    );
                    println!(
                        "Saved there as conflict {}; anyone can take it.",
                        short(conflict)
                    );
                    println!("  {url}/conflicts/{conflict}");
                    std::process::exit(3);
                }
                Some(Proposal::Rejected { change, evidence }) => {
                    println!(
                        "Did not land {}: a check failed on {remote}'s combined result.",
                        short(change)
                    );
                    for e in evidence.iter().filter(|e| !e.ok) {
                        println!("  FAIL {}\n{}", e.check, indent(&e.detail, "       "));
                    }
                    std::process::exit(2);
                }
            }
        }
        Cmd::Diff { change } => print!("{}", repo.diff_text(&ws, change.as_deref())?),
        Cmd::Revert { change } => report(repo.revert(&ws, &change, &actor)?, cli.json)?,
        Cmd::Pick { change } => report(repo.pick(&ws, &change, &actor)?, cli.json)?,
        Cmd::Restore { paths } => {
            let done = repo.restore(&ws, &paths)?;
            if done.is_empty() {
                println!("Nothing to restore.");
            } else {
                println!("Restored {}", done.join(", "));
            }
        }
        Cmd::Shelve { name } => {
            let n = repo.shelve(&ws, &name, &actor)?;
            println!("Shelved {n} edit(s) as {name}; the workspace is back at its base.");
        }
        Cmd::Unshelve { name } => {
            let n = repo.unshelve(&ws, &name, &actor)?;
            println!("Brought back {n} edit(s) from {name}.");
        }
        Cmd::Shelves => {
            for (name, h) in &refs.shelves {
                let c = repo.change(h)?;
                println!("{name:<16} {} edit(s) by {}", c.ops.len(), c.actor);
            }
        }
        Cmd::Tag { name: None, .. } => {
            let keys = repo.config()?.keys;
            for (name, h) in &refs.tags {
                let signed = match refs.tag_signatures.get(name) {
                    None => "unsigned".to_string(),
                    Some(sig) => match grove_core::trust::verify_tag(name, h, sig, &keys) {
                        Ok(who) => format!("signed by {who}"),
                        Err(why) => why,
                    },
                };
                println!("{name:<16} {}  {signed}", h.short());
            }
        }
        Cmd::Tag {
            name: Some(name),
            delete: true,
            ..
        } => {
            repo.untag(&name, &actor)?;
            println!("Removed tag {name}");
        }
        Cmd::Tag {
            name: Some(name),
            at,
            ..
        } => {
            let h = repo.tag(&name, at.as_deref(), &actor)?;
            println!("Tagged {} as {name}", h.short());
        }
        Cmd::Lane(LaneCmd::New { name, from }) => {
            let h = repo.lane_new(&name, from.as_deref(), &actor)?;
            println!(
                "Started lane {name} at {}. Work on it with `grove lane use {name}`.",
                h.short()
            );
        }
        Cmd::Lane(LaneCmd::Use { name }) => {
            let lane = if name == "main" {
                None
            } else {
                Some(name.as_str())
            };
            repo.lane_use(&ws, lane, &actor)?;
            println!("{ws} now works on {name}.");
        }
        Cmd::Lane(LaneCmd::Land { name }) => report(repo.lane_land(&name, &actor)?, cli.json)?,
        Cmd::Lane(LaneCmd::List) => {
            println!("  main             {}", repo.main()?.short());
            for (name, h) in &refs.lanes {
                let here = if refs.workspaces.get(&ws).and_then(|w| w.lane.as_deref())
                    == Some(name.as_str())
                {
                    "*"
                } else {
                    " "
                };
                println!("{here} {name:<16} {}", h.short());
            }
        }
        Cmd::Lane(LaneCmd::Drop { name }) => {
            repo.lane_drop(&name, &actor)?;
            println!("Dropped lane {name}");
        }
        Cmd::Find { query } => {
            for (file, line, text) in repo.find(&query)? {
                println!("{file}:{line}: {}", text.trim());
            }
        }
        Cmd::Evolve { change } => {
            for c in repo.evolve(&change)? {
                println!(
                    "v{}  {}  {} op(s)  \"{}\"",
                    c.version,
                    short(&c.id),
                    c.ops.len(),
                    c.intent
                );
            }
        }
        Cmd::Shortlog => {
            for (who, n) in repo.shortlog()? {
                println!("{n:>5}  {who}");
            }
        }
        Cmd::Bisect { run, good } => {
            let b = repo.bisect(&run, good.as_deref())?;
            match b.change {
                Some(c) => println!(
                    "First failing landing: {} {} \"{}\" by {} ({} tested)",
                    b.first_bad.short(),
                    short(&c.id),
                    c.intent,
                    c.actor,
                    b.tested
                ),
                None => println!(
                    "First failing landing: {} ({} tested)",
                    b.first_bad.short(),
                    b.tested
                ),
            }
        }
        Cmd::Git(GitCmd::Pull { remote }) => {
            let p = grove_core::git::pull(&repo, &remote, &actor)?;
            println!(
                "Pulled from {remote}: {} fast-forward, {} replayed on top of Grove's main.",
                p.fast_forward, p.replayed
            );
            if let Some((sha, why)) = p.conflict {
                println!(
                    "Stopped at {}: {why}. Resolve it, then pull again.",
                    &sha[..sha.len().min(10)]
                );
            }
        }
        Cmd::Key(KeyCmd::New) => {
            let k = grove_core::trust::new_key(&actor)?;
            println!("Signing key for {actor}: {k}");
            println!("Trust it here with `grove key trust`.");
        }
        Cmd::Key(KeyCmd::Show) => match grove_core::trust::public_key(&actor) {
            Some(k) => println!("{k}"),
            None => bail!("{actor} has no key yet: `grove key new`"),
        },
        Cmd::Key(KeyCmd::Trust) => {
            let k = grove_core::trust::public_key(&actor)
                .ok_or_else(|| anyhow::anyhow!("{actor} has no key yet: `grove key new`"))?;
            let spec = grove_core::trust::actor_spec(&actor);
            repo.update_config(|c| {
                c.keys.insert(spec.clone(), k.clone());
                Ok(())
            })?;
            println!("This repository now trusts {spec}'s key {}", &k[..12]);
        }
        Cmd::Approve { change } => {
            let c = repo.approve(&change, &actor)?;
            println!(
                "{} approved {} v{} \"{}\"",
                actor.name,
                short(&c.id),
                c.version,
                c.intent
            );
        }
        Cmd::Note { change, text } => {
            repo.note(&change, &text, &actor)?;
            println!("Noted.");
        }
        Cmd::Who => {
            for p in repo.who()? {
                let who = p.actor.map(|a| a.to_string()).unwrap_or_default();
                let lane = p.lane.map(|l| format!(" on {l}")).unwrap_or_default();
                let what = match p.draft {
                    Some((c, names)) => format!(
                        "{} \"{}\" writes {}",
                        short(&c.id),
                        c.intent,
                        names.join(", ")
                    ),
                    None if p.unrecorded > 0 => format!("{} unrecorded edit(s)", p.unrecorded),
                    None => "idle".into(),
                };
                println!("{:<12} {:<26} {}{}", p.workspace, who, what, lane);
            }
        }
        Cmd::Config { key: None, .. } => {
            println!("{}", serde_json::to_string_pretty(&repo.config()?)?)
        }
        Cmd::Config {
            key: Some(key),
            value,
        } => {
            let value =
                value.ok_or_else(|| anyhow::anyhow!("give a value: grove config {key} <value>"))?;
            let flag = || -> Result<bool> {
                value
                    .parse()
                    .map_err(|_| anyhow::anyhow!("{key} is true or false"))
            };
            let c = repo.update_config(|c| {
                match key.as_str() {
                    "review.require_signed" => c.review.require_signed = flag()?,
                    "review.signature" => c.review.signature = flag()?,
                    "review.capabilities" => c.review.capabilities = flag()?,
                    "review.hot_links" => c.review.hot_links = if value == "off" { None } else { Some(value.parse()?) },
                    "review.protected" => c.review.protected = value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                    _ => bail!("unknown setting {key}; try review.require_signed, review.signature, review.capabilities, review.hot_links, review.protected"),
                }
                Ok(())
            })?;
            println!("{}", serde_json::to_string_pretty(&c.review)?);
        }
        Cmd::Git(GitCmd::Pr { remote, change }) => {
            let c = match change {
                Some(p) => repo.find_any_change(&p)?,
                None => {
                    let id = refs
                        .workspaces
                        .get(&ws)
                        .and_then(|w| w.draft.clone())
                        .ok_or_else(|| {
                            anyhow::anyhow!("no draft here: record one with `grove new`")
                        })?;
                    repo.change(&refs.drafts[&id])?
                }
            };
            let pr = grove_core::git::pr(&repo, &remote, &c)?;
            match pr.url {
                Some(u) => println!("Opened {u}"),
                None => println!(
                    "Pushed {} as branch {}. Open a pull request from it on your git host.",
                    &pr.commit[..10],
                    pr.branch
                ),
            }
        }
        Cmd::Verify => {
            let (n, bad) = repo.verify()?;
            if bad.is_empty() {
                println!("All {n} objects match their addresses.");
            } else {
                bail!(
                    "{} of {n} objects don't match their addresses: {}",
                    bad.len(),
                    bad.join(", ")
                );
            }
        }
    }
    Ok(())
}

fn describe(op: &Op, base: &View) -> String {
    let name = |id: &str| {
        base.decls
            .get(id)
            .map(|n| n.entry.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    match op {
        Op::Add { name, file, .. } => format!("add      {name}  ({file})"),
        Op::Replace { id, .. } => format!("change   {}", name(id)),
        Op::Rename { from, to, .. } => format!("rename   {from} -> {to}"),
        Op::Delete { id, .. } => format!("delete   {}", name(id)),
    }
}

fn names(ids: &std::collections::BTreeSet<DeclId>, view: &View, ops: &[Op]) -> String {
    if ids.is_empty() {
        return "-".into();
    }
    ids.iter()
        .map(|id| {
            view.decls
                .get(id)
                .map(|n| n.entry.name.clone())
                .unwrap_or_else(|| {
                    ops.iter()
                        .find_map(|op| match op {
                            Op::Add { id: a, name, .. } if a == id => Some(name.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| id.clone())
                })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn indent(text: &str, pad: &str) -> String {
    text.trim_end()
        .lines()
        .map(|l| format!("{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Print what happened when a change tried to land.
fn report(outcome: LandOutcome, json: bool) -> Result<()> {
    match outcome {
        LandOutcome::Landed {
            change,
            mode,
            evidence,
            ..
        } => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"landed": change.id, "mode": mode, "evidence": evidence})
                );
            } else {
                println!(
                    "Landed {} \"{}\" ({})",
                    short(&change.id),
                    change.intent,
                    mode_word(&mode)
                );
                for e in &evidence {
                    println!(
                        "  {} {}{}",
                        if e.ok { "ok  " } else { "FAIL" },
                        e.check,
                        if e.detail.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", e.detail.lines().next().unwrap_or(""))
                        }
                    );
                }
            }
            Ok(())
        }
        LandOutcome::Rejected { change, evidence } => {
            println!(
                "Did not land {} \"{}\": {}",
                short(&change.id),
                change.intent,
                refusal(&change, &evidence)
            );
            for e in evidence.iter().filter(|e| !e.ok) {
                println!(
                    "  FAIL {}: {}",
                    e.check,
                    e.detail.lines().next().unwrap_or("")
                );
            }
            std::process::exit(2);
        }
        LandOutcome::Conflict {
            change,
            id,
            conflict,
        } => {
            println!(
                "Did not land {} \"{}\": it clashes with work already there.",
                short(&change.id),
                change.intent
            );
            for d in &conflict.decls {
                println!("  {}", d.name);
            }
            println!(
                "Saved as conflict {}. See it with `grove conflicts {}`.",
                short(&id),
                short(&id)
            );
            std::process::exit(3);
        }
    }
}

/// Why a landing was refused, in words.
fn refusal(change: &Change, evidence: &[Evidence]) -> String {
    let failed: Vec<&str> = evidence
        .iter()
        .filter(|e| !e.ok)
        .map(|e| e.check.as_str())
        .collect();
    if failed.iter().any(|c| c.starts_with("review")) {
        format!(
            "it needs a person's approval first (`grove approve {}`).",
            short(&change.id)
        )
    } else if failed.contains(&"signature") {
        "its signature isn't one this repository trusts (`grove key new`, `grove key trust`)."
            .into()
    } else {
        "a check failed on the combined result.".into()
    }
}
