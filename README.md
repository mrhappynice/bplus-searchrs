# bplus🤷🏻‍♂️ Searchrs 
v0.4.1.2 local db search + APIs!!  
Check out the [search-apis.md](https://github.com/mrhappynice/bplus-searchrs/blob/main/search-apis.md) file to see how to add your own. (When clicking the +Add button)Apple Podcasts API is auto-populated to demo how, remove to add custom paths.
#### Super small, super fast, Rust + HTML/JS UI. Local database of convos for research memory, provider and model selection(local and paid) , built-in native connectors and generic user added apis through UI. Optional connection to SearXNG. <sub>app might be a bit buggy :)</sub> added support in [bplus-TUI](https://github.com/mrhappynice/bplus-tui) 
---
### Quick Start🏁

- ```sh 
  curl -fsSL https://raw.githubusercontent.com/mrhappynice/bplus-searchrs/main/install.sh | bash
  ```
  enter ```bplus-searchrs``` folder and run: ```./bplus-searchrs``` - Run LM Studio, Ollama, etc(port 1234), put any keys for model providers in ```.env```
---  
- Windows terminal single paste install:
  ```sh
  $urls = @(
    "https://github.com/mrhappynice/bplus-searchrs/raw/refs/heads/main/.env",
    "https://github.com/mrhappynice/bplus-searchrs/releases/download/v0.4.1.2/bplus-searchrs-static.exe"
    "https://github.com/mrhappynice/bplus-searchrs/raw/refs/heads/main/blank.db"
  )

  foreach ($u in $urls) {
    $name = Split-Path $u -Leaf
    Invoke-WebRequest $u -OutFile $name
  }

  ```
  Simply run: ```.\bplus-searchrs.exe``` in terminal. Run LM Studio, Ollama, etc(port 1234), put any keys for model providers in ```.env``` Then connect to http://localhost:3001
  
---  
- Free local search and model providers(Openrouter, OAI, Google) with native search connectors and user added generic APIs. Debugger added to terminal output check this for help. 
- No MCP needed, custom backend, low context yayyyy
- ~10MB binary - UI is gargabe right now, <sub>help..</sub>
- SearXNG optional, connect to SearXNG instance or use built-in web search, edit providers to customize. Toggle on/off.
- dl
  - ```sh
    git clone https://github.com/mrhappynice/bplus-searchrs.git && cd bplus-searchrs
    ```
- Build it:
  - ```sh
    cargo build --release
    ```
    - If you want MSVC statically linked build on Windows(to avoid missing dll errors) Before you build - create the folder: ```.cargo``` in the bplus-searchrs main directory and copy the config.toml from the web folder into it. Test with dumpbin /dependents xxxx.exe
- Run:
  - ```sh
    ./run-bps.sh
    ```

- Model run example, download latest llama.cpp compatible version with your system and:  
    *Note, the Local provider drop down is listening on port 1234 so just run any OAI compat endpoint on that port.

  - ```sh
    ./llama-server -m Qwen3-0.6B-Q8_0.gguf -c 8000 -ngl 99 --port 1234
    ```
    or:
    ```sh
    ./llama-server -hf unsloth/Qwen3-0.6B-GGUF:Q8_0 -c 8000 -ngl 99 --port 1234

    ```
