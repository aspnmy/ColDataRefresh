# ColDataRefresh SSD Cold Data Maintenance System v5.0.0
Intelligently detects cold data on SSD and solves the cold data crash problem with data validation.

## v5.0.0 Update Content
- Completely rewritten in Rust language, providing higher performance and reliability
- Retained all original features with performance optimizations
- Supports concurrent processing to improve data refresh speed
- Enhanced cross-platform compatibility, supporting Windows and Linux
- Optimized file system operations to reduce I/O overhead
- Improved error handling mechanism to enhance program stability
- Simplified build process using Cargo for dependency management and building

## v4.7.0 Update Content (Historical Version)
- Fixed the `full_refresh_file` mode to ensure data is correctly written to disk
- Fixed PyInstaller build script issues to ensure dependency files are correctly packaged
- Ensured logs are saved in the program directory instead of temporary directories for easier querying
- Implemented a complete full refresh business flow, including file backup/restore and space filling
- Optimized the full refresh business flow:
  - First attempts formatting operation, then falls back to file deletion if formatting fails
  - TRIM operation is executed at the end to avoid blocking intermediate processes
- Added automatic admin privilege elevation to simplify user operations
- Optimized TRIM operations based on Windows version:
  - Windows 11: Executes ReTrim + SlabConsolidate + ReTrim combination operation
  - Windows 10: Only executes ReTrim operation
  - Windows 10 and below: Uses DeviceIoControl method to execute TRIM operations
- Added duplicate TRIM operation avoidance mechanism to improve efficiency
- Enhanced TRIM operation user prompts to inform users about operation details and precautions
- Fixed SSL certificate verification failure issue to ensure the program can run normally in various environments
- Enhanced logging and error handling

### What is Cold Data
Cold data refers to data that has been stored on the hard drive for a long time (e.g., half a year or even longer) and has not been rewritten or updated, which is intuitively expressed in terms of files, but in reality is reflected in the physical level of the corresponding storage unit of the file. Usually, documents, videos, music, pictures and other static data stored on the hard drive for a long time are cold data, and even any files that have been read by the operating system, programmes and games over a long period of time without modification or update will ‘grow’ to be cold data in the future (hot or incremental updates are already very mature nowadays, but they can be used for a long time). Generally speaking, updates to systems, games, and applications will only update the parts that need to be changed, and leave the parts that don't need to be changed untouched).
**Note that the formation of cold data is only related to writing, not reading, even if a file is read frequently, but not modified to write, it is possible to become cold data** (this is also the reason why some people react to the slow loading of the games that they often play because of the cold data falling speed).

### What problems can cold data cause
Cold data on an SSD can cause slow read speeds, and in extreme cases, even unreadable.

> Most SSD firmwares, like Samsung's, will move cold data around to ‘warm it up’ during idle periods, but some manufacturers' firmwares do not have this feature. This is why this tool was developed.
> Note: The Trim function/defragmentation of SSDs does not alleviate the slowdown of cold data reading.

### How to determine/resolve the cold data read dropout problem of my hard drive

The easiest thing to do is to find a file that has been lying on your hard drive for a long time (e.g. more than two years) and has not been modified, copy it to another hard drive, and observe whether the copying speed has dropped?
Copy the file back, and the problem is solved (because the file becomes ‘newly’ written and is no longer cold data).

You can also use this tool, which will automatically determine if your file is cold or not.

### Features of this tool/differences with `DiskFresh` and other tools

1. `DiskFresh` is also designed to deal with cold data, but DiskFresh is based on the more underlying `Sector` level of the disc to do a full overwrite. The disadvantage is that it takes a long time to refresh, and will refresh unnecessary non-cold data blocks, which may reduce the life of the hard disc; **This tool is based on the file system level, and only refreshes the detected cold data, and comes with CRC file checksum, which is safer and faster. **,
2. This tool supports saving the file refresh progress, you can exit at any time and continue the data refresh operation the next time
3. This tool is open source.
4. Developed in Rust language, providing higher performance and reliability

### How to use

> **Please right click the programme - `Run as administrator` **, this is necessary, you can not grant permission, but specific files may be accessed or overwrite failed.

1. **Build from source code**:
   - Ensure Rust development environment is installed (recommended to use rustup for installation)
   - Clone the repository: `git clone https://github.com/aspnmy/ColDataRefresh.git`
   - Switch to v5.0 branch: `git checkout v5.0`
   - Enter project directory: `cd ColDataRefresh/coldatafresh`
   - Build the project: `cargo build --release`
   - Run the program: `cargo run --release` or directly run the generated executable file

2. The program provides three modes:
   - Smart mode: Automatically detects and refreshes cold data, preserving the original file content
   - Full disk cold data activation mode: Replaces file content with specific values, **this mode will cause file content loss, use with caution! **
   - TRIM mode: Notifies SSD which data blocks are invalid, improves write performance and extends SSD life

3. Enter the directory you want to scan for cold data, e.g. `D:\DL` or the whole hard drive `D:\` (Windows users can select the folder and press `Ctrl+Shift+C` to copy the directory address), press enter.

4. Enter the number of days of cold data, e.g. `300`, the programme will scan files that have been last modified more than 300 days ago. (Entering 0 will scan all files in the directory.) Press Enter to run the program.

5. **Important: If you need to exit the programme while it is running, please press `Ctrl+C` on the console first to send the terminate command, otherwise it may cause data loss! **

### TRIM Function Description

TRIM is an advanced SSD maintenance feature that can significantly improve write performance and extend SSD life by notifying the solid-state drive which data blocks are no longer valid. The TRIM function implemented by this tool:

- Communicates directly with SSD via operating system API, more efficient than file system level TRIM
- Supports Windows and Linux platforms
- Automatically applies TRIM commands to relevant data blocks during data refresh
- No formatting or low-level operations required, safe and reliable

> Note: TRIM functionality requires hardware and operating system support, please ensure your SSD and operating system support TRIM commands.
