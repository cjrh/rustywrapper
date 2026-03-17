# Snaxum library development commands

# Absolute path to venv for PyO3
venv_dir := justfile_directory() / ".venv"
venv_python := venv_dir / "bin/python"

# Export environment variables for PyO3 and Python paths
export PYO3_PYTHON := venv_python
python_version := shell('/usr/bin/python3 -c "import sys; print(str(sys.version_info.major) + chr(46) + str(sys.version_info.minor))"')
export PYTHONPATH := venv_dir / "lib" / ("python" + python_version) / "site-packages"

@default:
    just --list

# Python venv management
#
# This is a pickle. PyO3 works great with venvs, but only if the libpython
# shared library is in a standard location. If you use a system
# Python to create the venv, libpython nevertheless remains in a standard
# location and at runtime the linker has no problem finding it.
# The problem arises when you use a different python distribution
# that does not have libpython accessible in a standard location.
# We're using uv, which has this problem. If we used uv to create
# the venv, then at runtime PyO3 would not be able to find libpython
# We could resolve the issue by setting LD_LIBRARY_PATH, but that
# is messy and error-prone. So instead we create the venv
# using the system Python, which works fine. All the other commands
# nevertheless use uv.
venv:
    /usr/bin/python3 -m venv --without-pip .venv

# Full dev setup from scratch
setup: venv
    @echo "Development environment ready!"

# Build the library
build: venv
    PYO3_PYTHON={{venv_python}} cargo build --lib
    cargo build --lib
