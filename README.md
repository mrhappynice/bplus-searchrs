# bplus🤷🏻‍♂️ Search 
v0.3
### Local LLM powered private web search built in and with optional connection to SearXNG instance.

---

- Free search API providers. 
- No MCP needed, custom backend, low context yayyyy
- SearXNG optional, connect to SearXNG instance or use built-in web search, edit providers to customize
- Run LM Studio, Ollama, etc(port 1234 and creds in .env) then run this
- dl
  - ```sh
    git clone https://github.com/mrhappynice/bplus-search.git && cd bplus-search
    ```
- Install: 
  - ```sh
    npm install
    ```
- Build it:
  - ```sh
    chmod +x build-sea.sh run-bps.sh
    ./build-sea.sh
    ```
- Run:
  - ```sh
    ./run-bps.sh
    ```
- You just need the ```bplus-search``` executable with the ```node_modules``` folder 

- Model run example, download latest llama.cpp compatible version with your system and:
  - ```sh
    ./llama-server -m Qwen3-0.6B-Q8_0.gguf -c 8000 -ngl 99 --port 1234
    ```
    or:
    ```sh
    ./llama-server -hf unsloth/Qwen3-0.6B-GGUF:Q8_0 -c 8000 -ngl 99 --port 1234

    ```
