# Remote Debugging tuliprox in VSCode with Docker

This guide explains how to set up remote debugging for the tuliprox project in VSCode using a Docker container.

## Prerequisites

- VSCode with the following extensions:
  - [CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb)
  - [Docker](https://marketplace.visualstudio.com/items?itemName=ms-azuretools.vscode-docker)
- Docker and Docker Compose installed
- Rust toolchain (if building locally)

## Setup

1. Copy the debug configuration files to your project root:
   ```bash
   cp -r docker/debug/.vscode .
   cp docker/debug/docker-compose.debug-override.yml .
   ```

## Configuration

### Docker Compose
The `docker-compose.debug-override.yml` file:
- Configures LLDB debug ports (10586 for control, 10600-10700 for sessions)
- Sets up privileged mode for debugging
- Mounts cargo cache volumes for faster builds
- Uses the debug build target

### VSCode Tasks
The `.vscode/tasks.json` defines:
- `docker-compose-up-debug`: Builds and starts containers with debug config
- `docker-compose-down`: Stops and removes containers

### VSCode Launch Configurations
The `.vscode/launch.json` provides two debug configurations:
1. **Docker Remote Debug (start & attach)**:
   - Automatically starts containers and attaches debugger
   - Uses the `docker-compose-up-debug` pre-launch task
2. **Docker Remote Debug (attach only)**:
   - Only attaches debugger to running container

## Debugging Workflow

1. Open the project in VSCode
2. Set breakpoints in your Rust code
3. Select the debug configuration from the Run and Debug panel:
   - Use "Docker Remote Debug (start & attach)" for a complete start-to-debug workflow
   - Use "Docker Remote Debug (attach only)" if containers are already running
4. Press F5 to start debugging

## Troubleshooting

### Common Issues

1. **Debugger fails to connect**:
   - Verify ports 10586 and 10600-10700 are available
   - Check Docker container logs for errors
   - Ensure the container is running in privileged mode

2. **Breakpoints not hitting**:
   - Verify source mapping is correct in launch.json
   - Ensure you're using the debug build (`target/debug/tuliprox`)

3. **Performance issues**:
   - The debug build is slower than release
   - Consider adding more RAM to Docker if needed

### Debugging Tips

- Use the VSCode debug console for LLDB commands
- The debugger supports all standard LLDB features (watch, call stack, etc.)
- Breakpoints can be set while the program is running