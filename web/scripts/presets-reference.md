## Presets — `[presets.*]`

Presets are **named bundles of agent overrides** that you can switch between with a
single top-level key. Unlike the sections above, `[presets.*]` does **not** appear in the
generated reference: presets are only ever *materialized* into the merged configuration
when one is selected, so `atelier --print-config` shows the resulting `[agents.*]` values,
never the preset definitions themselves. This section is therefore hand-written and lives
alongside the generated reference.

### Selecting a preset

Set the top-level `preset` key to the name of the preset you want active:

```toml
preset = "fast"
```

A preset is applied **before** your local `[agents.*]` overrides, so a value you set
directly on an agent always wins over the same value coming from the preset. The
precedence, lowest to highest, is: built-in defaults → home config → active preset →
local `[agents.*]` → CLI flags.

### Defining a preset

Each preset is a table under `[presets.<name>.agents.<agent>]`. Inside it you may override
any field of an agent profile — `runtime`, `model`, `model_fallbacks`, `effort`,
`thinking`, `capabilities`, `tools`, the prompt-source fields (`instructions`,
`instructions_file`, `instructions_append_file`), `orchestrator_description`, and
`enabled`:

```toml
# A "fast" preset that swaps the fixer onto a lighter model with a minimal
# reasoning effort, leaving every other agent at its default.
[presets.fast.agents.fixer]
model = "default"
model_fallbacks = ["glm-5.1"]
effort = "minimal"

# Presets may touch any number of agents.
[presets.fast.agents.reviewer]
effort = "medium"
```

You can define multiple presets in the same file and switch between them by changing the
single `preset` key — only the selected preset's overrides are applied; the others are
inert. Selecting a different preset never leaks fields from a previously defined preset.

> Council review presets are a separate mechanism: they live under
> `[council.presets.*]` (documented in the **Council** section above) and configure the
> serial review workflow, not the main agent roster.
