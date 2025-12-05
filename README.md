# Rufi

A fast, native macOS application launcher inspired by Rofi. Rufi provides a spotlight-like interface for quickly searching and launching applications, files, and system commands.

## Features

- **Application Search**: Quickly find and launch installed applications
- **File Search**: Browse and open files from your system
- **Command Mode**: Execute system commands directly
- **Fuzzy Matching**: Smart search algorithm finds what you're looking for
- **Customizable**: Configure appearance and behavior via JSON config
- **Native Performance**: Built in Rust with macOS native frameworks

## Installation

### Build from Source

```bash
# Clone the repository
git clone <repository-url>
cd rufi

# Build the release binary
cargo build --release

# The binary will be located at target/release/rufi
```

## Usage

Run Rufi from the terminal:

```bash
./target/release/rufi
```

### Search Modes

- **Apps Mode**: Search through installed applications
- **Files Mode**: Search through your file system
- **Run Mode**: Execute system commands

Navigate between modes and select items using keyboard shortcuts (configured in your config file).

## Configuration

Rufi stores its configuration in `~/.config/rufi/config.json`. The configuration file allows you to customize:

- Window appearance
- Keyboard shortcuts
- Search behavior
- And more

## Requirements

- macOS (uses native Cocoa frameworks)
- Rust toolchain for building

## Dependencies

- cocoa - macOS UI framework bindings
- objc - Objective-C runtime
- core-foundation - Core Foundation framework
- core-graphics - Core Graphics framework
- serde/serde_json - Configuration serialization
- dirs - System directory access
- fuzzy-matcher - Fuzzy string matching
- rand - Random number generation

## License

[Add your license here]

## Contributing

[Add contribution guidelines here]
