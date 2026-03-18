# Chimera library development commands

# Absolute path to venv for PyO3
venv_dir := justfile_directory() / ".venv"
venv_python := venv_dir / "bin/python"

# Export environment variable for PyO3 build-time Python selection
export PYO3_PYTHON := venv_python

@default:
    just --list

# Create venv with system Python (required for PyO3 shared libpython).
# See PYTHON_CONFIGURATION.md for details on Python distribution requirements.
venv:
    /usr/bin/python3 -m venv --without-pip .venv

# Full dev setup from scratch
setup: venv
    @echo "Development environment ready!"

# Build the library
build: venv
    PYO3_PYTHON={{venv_python}} cargo build --lib
    cargo build --lib
