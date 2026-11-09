# claude-manager

## Definition of done

Before handing a change to the user for verification:

1. `cargo test` and, for frontend changes, `npm --prefix crates/app/frontend test` and `(cd crates/app/frontend && npx tsc --noEmit)`.
2. For anything the UI shows, use the `verify-ui` skill: exercise the change in the hidden agent app (`scripts/agent-app.sh`, `scripts/ui`), look at the screenshots, check `scripts/ui logs`, and fix what you find.
3. Report what you exercised and attach the final screenshot paths; list only what the agent app cannot check.

Never run `make dev` or otherwise launch the user's own window; the `CM_AGENT` copy is the only instance you start.
