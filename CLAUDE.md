# claude-manager

## Definition of done

Before handing a change to the user for verification:

1. `cargo test`, `bash scripts/tests/guided_test.sh` (both run by `make test`) and, for frontend changes, `npm --prefix crates/app/frontend test` and `(cd crates/app/frontend && npx tsc --noEmit)`.
2. For anything the UI shows, use the `verify-ui` skill: exercise the change in the hidden agent app (`scripts/agent-app.sh`, `scripts/ui`), look at the screenshots, check `scripts/ui logs`, and fix what you find.
3. Report what you exercised and attach the final screenshot paths; list only what the agent app cannot check.
4. Hand off to the user in the guided dev window per the `verify-ui` skill ("Hand off to the user"): sandbox, prepared sessions, one step per message. Skip it for backend-only changes; for real-data-only changes give plain `make dev` steps instead.

Never run `make dev` or `make dev-guided`, or otherwise launch the user's own window; the `CM_AGENT` copy is the only instance you start.
