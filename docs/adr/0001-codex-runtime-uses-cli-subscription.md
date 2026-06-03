# Codex runtime uses CLI subscription auth

The first Codex integration launches Codex as a child CLI process rather than calling OpenAI APIs directly. This preserves the user's existing Codex subscription authentication path and keeps it distinct from API-key runtimes such as Z.ai; a direct OpenAI API runtime can be added later as a separate execution runtime.
