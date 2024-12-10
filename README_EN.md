# ColDataRefresh
Intelligently detects cold data on SSD and solves the cold data crash problem with data validation.

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

### How to use

> **Please right click the programme - `Run as administrator` **, this is necessary, you can not grant permission, but specific files may be accessed or overwrite failed.

1. Releases interface has compiled exe binaries, download and double click to run / you can also run from python source code (you can change more configurations in the code)
2. Enter the directory you want to scan for cold data, e.g. `D:\DL` or the whole hard drive `D:\` (Windows users can select the folder and press `Ctrl+Shift+C` to copy the directory address), press enter.
3. Enter the number of days of cold data, e.g. `300`, the programme will scan files that have been last modified more than 300 days ago. (Entering 0 will scan all files in the directory.) Press Enter to run the program.
4. **Important: If you need to exit the programme while it is running, please press `Ctrl+C` on the console first to send the terminate command, otherwise it may cause data loss! **Important.

### Program screenshots Screenshots
! [projectimage](. /projectimage.png)