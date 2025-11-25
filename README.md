# bplus🤷🏻‍♂️ Searchrs
v0.4.1.1 grrr APIs!!  
Check out the [search-apis.md](https://github.com/mrhappynice/bplus-searchrs/blob/main/search-apis.md) file to see how to add your own. (When clicking the +Add button)Apple Podcasts API is auto-populated to demo how, remove to add custom paths. 
### Local LLM powered private web search built in and with optional connection to SearXNG instance.  

---

- Free local search and API providers. 
- No MCP needed, custom backend, low context yayyyy
- SearXNG optional, connect to SearXNG instance or use built-in web search, edit providers to customize. Remove ```USE_NATIVE=1``` from ```.env``` to use SearXNG instead of built-in.
- Run LM Studio, Ollama, etc(port 1234 and creds in .env) then run this
- dl
  - ```sh
    git clone https://github.com/mrhappynice/bplus-searchrs.git && cd bplus-searchrs
    ```
- Build it:
  - ```sh
    cargo build --release
    ```
- Run:
  - ```sh
    ./run-bps.sh
    ```
- ~10MB binary 

- Model run example, download latest llama.cpp compatible version with your system and:  
    *Note, the Local provider drop down is listening on port 1234 so just run any OAI compat endpoint on that port.

  - ```sh
    ./llama-server -m Qwen3-0.6B-Q8_0.gguf -c 8000 -ngl 99 --port 1234
    ```
    or:
    ```sh
    ./llama-server -hf unsloth/Qwen3-0.6B-GGUF:Q8_0 -c 8000 -ngl 99 --port 1234

    ```
