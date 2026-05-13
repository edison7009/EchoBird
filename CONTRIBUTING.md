# Contributing to EchoBird

Thank you for your interest in contributing to EchoBird! This guide will help you get started.

## Development Environment

### Prerequisites

- **Node.js**: >= 20.0.0 (tested with v24.12.0)
- **npm**: >= 10.0.0 (tested with v11.12.1)
- **Rust**: >= 1.70.0 (tested with v1.93.1)
- **Cargo**: >= 1.70.0 (tested with v1.93.1)

### System Dependencies

#### Windows
- Visual Studio 2022 with C++ build tools
- WebView2 (usually pre-installed on Windows 10/11)

#### macOS
```bash
xcode-select --install
```

#### Linux (Ubuntu/Debian)
```bash
sudo apt-get update
sudo apt-get install -y \
  libgtk-3-dev \
  libwebkit2gtk-4.0-dev \
  libappindicator3-dev \
  librsvg2-dev \
  patchelf
```

## Quick Start

### 1. Clone the Repository

```bash
git clone https://github.com/edison7009/EchoBird.git
cd EchoBird
```

### 2. Install Dependencies

```bash
npm install
```

### 3. Run Development Server

```bash
npm run dev
```

This will:
- Sync tools to the build output directory
- Start the Tauri development server
- Open the application window

### 4. Build for Production

```bash
npm run build
```

The built application will be in `src-tauri/target/release/bundle/`.

## Project Structure

```
EchoBird/
├── src/                      # Frontend source (React + TypeScript)
│   ├── api/                  # Tauri command bindings
│   ├── components/           # Reusable UI components
│   ├── pages/                # Main application pages
│   │   ├── MotherAgent/      # AI chat interface
│   │   ├── ModelNexus/       # Model management
│   │   ├── LocalServer/      # Local LLM server
│   │   ├── AiPulse/          # AI news feed
│   │   └── AppManager/       # Tool installation
│   ├── stores/               # Zustand state management
│   ├── hooks/                # Custom React hooks
│   ├── i18n/                 # Internationalization
│   └── utils/                # Utility functions
├── src-tauri/                # Rust backend (Tauri)
│   ├── src/
│   │   ├── commands/         # Tauri command handlers
│   │   ├── services/         # Core business logic
│   │   │   ├── agent_loop.rs      # AI agent orchestration
│   │   │   ├── llm_client.rs      # LLM API client
│   │   │   ├── local_llm/         # Local LLM server
│   │   │   ├── model_manager.rs   # Model configuration
│   │   │   └── tool_manager.rs    # Tool installation
│   │   ├── models/           # Data models
│   │   └── utils/            # Utility functions
│   ├── Cargo.toml            # Rust dependencies
│   └── tauri.conf.json       # Tauri configuration
├── tools/                    # Bundled tools and scripts
│   ├── codex/                # Codex CLI integration
│   └── ...                   # Other tools
├── public/                   # Static assets
│   └── tools/                # Embedded tool HTML
├── .github/
│   └── workflows/
│       ├── ci.yml            # CI quality checks
│       └── release.yml       # Release automation
└── package.json              # Frontend dependencies
```

## Development Workflow

### Code Quality Checks

Before committing, ensure all checks pass:

```bash
# Frontend checks
npm run typecheck       # TypeScript type checking
npm run format:check    # Prettier formatting
npm run lint            # ESLint code quality
npm test                # Vitest unit tests

# Rust checks
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

### Auto-fix Issues

```bash
# Frontend
npm run format          # Auto-fix formatting
npm run lint:fix        # Auto-fix ESLint issues

# Rust
cargo fmt --manifest-path src-tauri/Cargo.toml
```

### CI Pipeline

All pull requests must pass CI checks:
- ✅ TypeScript type checking
- ✅ Prettier formatting
- ✅ ESLint (max 50 warnings)
- ✅ Vitest tests
- ✅ Rust formatting
- ✅ Clippy (zero warnings)
- ✅ Rust tests

## Commit Guidelines

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>: <description>

[optional body]
```

### Types

- `feat`: New feature
- `fix`: Bug fix
- `refactor`: Code refactoring
- `docs`: Documentation changes
- `test`: Test additions or fixes
- `chore`: Build process or tooling changes
- `perf`: Performance improvements
- `ci`: CI/CD changes

### Examples

```bash
feat(model): add support for Claude 4.7
fix(agent): prevent infinite loop in tool execution
refactor(ui): extract ModelCard component
docs(readme): update installation instructions
test(llm): add unit tests for streaming responses
chore(deps): update tauri to 2.10.1
```

## Pull Request Process

1. **Fork the repository** and create a feature branch
   ```bash
   git checkout -b feat/your-feature-name
   ```

2. **Make your changes** following the code style guidelines

3. **Run all quality checks** (see above)

4. **Commit your changes** with conventional commit messages

5. **Push to your fork** and create a pull request
   ```bash
   git push origin feat/your-feature-name
   ```

6. **Fill out the PR template** with:
   - Summary of changes
   - Test plan
   - Screenshots (if UI changes)

7. **Wait for review** — maintainers will review and provide feedback

## Code Style Guidelines

### TypeScript/React

- Use functional components with hooks
- Prefer `const` over `let`
- Use TypeScript strict mode
- Avoid `any` types when possible
- Use meaningful variable names
- Keep components small and focused (<200 lines)
- Extract reusable logic into custom hooks

### Rust

- Follow Rust naming conventions
- Use `Result<T, E>` for error handling
- Prefer immutable data structures
- Keep functions small and focused (<50 lines)
- Add doc comments for public APIs
- Use `clippy` suggestions

### General

- **Immutability**: Always create new objects, never mutate existing ones
- **Error handling**: Handle errors explicitly at every level
- **Input validation**: Validate all user input at system boundaries
- **No hardcoded secrets**: Use environment variables or secure storage
- **Comments**: Only add comments when the WHY is non-obvious

## Testing

### Frontend Tests

```bash
npm test                # Run all tests
npm test -- --watch     # Watch mode
npm test -- --coverage  # Coverage report
```

### Rust Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml -- --nocapture  # Show output
```

### Test Coverage

- Aim for **80%+ coverage** for new code
- Write tests for:
  - Core business logic
  - API endpoints
  - Error handling paths
  - Edge cases

## Debugging

### Frontend

1. Open DevTools in the app: `Ctrl+Shift+I` (Windows/Linux) or `Cmd+Option+I` (macOS)
2. Check console for errors
3. Use React DevTools extension

### Rust

1. Add debug prints:
   ```rust
   println!("Debug: {:?}", variable);
   ```

2. Use `RUST_LOG` environment variable:
   ```bash
   RUST_LOG=debug npm run dev
   ```

3. Use `rust-lldb` or `rust-gdb` for debugging

## Security

- **Never commit secrets** (API keys, passwords, tokens)
- **Validate all user input** before processing
- **Use parameterized queries** to prevent SQL injection
- **Sanitize HTML** to prevent XSS attacks
- **Report security issues** privately to the maintainers

## Getting Help

- **GitHub Issues**: Report bugs or request features
- **GitHub Discussions**: Ask questions or share ideas
- **Documentation**: Check the [README](README.md) and code comments

## License

By contributing to EchoBird, you agree that your contributions will be licensed under the MIT License.

---

Thank you for contributing to EchoBird! 🎉
