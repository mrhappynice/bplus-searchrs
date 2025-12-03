### Win build -

If you want MSVC statically linked build on Windows(to avoid missing dll errors) Before you build - create the folder: .cargo in the bplus-searchrs main directory and copy the config.toml from the web folder into it. Test with dumpbin /dependents xxxx.exe
