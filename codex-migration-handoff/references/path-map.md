# Codex Migration Path Map

## Mac Source

- `~/.codex`: primary Codex state, conversations, logs, sessions, memories, skills, plugins, generated images, automations, config, auth files.
- `~/Library/Application Support/Codex`: desktop app Chromium/profile data.
- `~/Library/Application Support/com.openai.codex`: app support data.
- `~/Library/Application Support/OpenAI/Codex`: OpenAI/Codex app support data.
- `~/Library/Caches/...`: optional caches; useful but not required.
- `~/Library/Preferences/*.plist`: Mac-only preferences; archive for completeness but do not restore directly to Windows.

## Windows Target

- `%USERPROFILE%\.codex`: primary Codex state.
- `%APPDATA%\Codex`: desktop app roaming data.
- `%APPDATA%\com.openai.codex`: app support data.
- `%APPDATA%\OpenAI\Codex`: OpenAI/Codex app support data.
- `%LOCALAPPDATA%\...`: optional cache equivalents.

## Project Continuity

Codex conversations may reference absolute source paths. Copy project folders separately or include them in the package under `projects/`. On Windows, reopen the project folder in Codex from its new path, for example:

- Mac: `/Users/caleb/Documents/New project`
- Windows: `C:\Users\Administrator\Documents\New project`

