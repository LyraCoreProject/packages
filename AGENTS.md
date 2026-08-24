# AGENTS.md

- Each visible top-level directory is one Package. Do not add a registry or manifest beside them.
- Package Rust compiles inside LyraCore's Module. Follow LyraCore's `CODING_STANDARDS.md` and
  `CONTEXT.md` at the core revision the Package targets.
- Treat Package code as trusted core code. Do not claim that Module sandboxing restricts one Package
  from other Module state.
- Do not commit Blizzard client files, DBC files, MPQ archives, credentials, or private Package
  source.
- Run `./.github/check-core-tip.sh /path/to/LyraCore` before submitting Package changes.
