Since you built a "Generic API" system, you can connect to **any** service that returns JSON. While major search engines (Google/Bing) require keys, there are many public data APIs and open-source search instances you can use for free.

Here are 3 distinct types of APIs to test your system:

### 1. iTunes Search (Podcasts & Media)
This is a very reliable, fast API from Apple that requires no key. It's great for testing if your system correctly handles titles, URLs, and descriptions.

*   **Name:** `Apple Podcasts`
*   **API URL:** `https://itunes.apple.com/search?term={q}&entity=podcast&limit=5`
*   **Headers:** `{}` (Leave empty)
*   **Results Path:** `results`
*   **Title Path:** `collectionName`
*   **URL Path:** `collectionViewUrl`
*   **Content Path:** `artistName`

### 2. TVMaze (TV Shows)
A free, open API for TV metadata. This tests your system's ability to handle nested JSON paths (e.g., extracting `show.name`).

*   **Name:** `TV Shows`
*   **API URL:** `https://api.tvmaze.com/search/shows?q={q}`
*   **Headers:** `{}`
*   **Results Path:** *Leave Empty* (The API returns a root array `[...]`, so we don't need to dig into a "results" field).
*   **Title Path:** `show.name`
*   **URL Path:** `show.url`
*   **Content Path:** `show.summary`

### 3. Public SearXNG Instance (Real Web Search)
Since `SearXNG` is open source, many people host public instances. You can hijack their JSON API to get real Google/Bing results without a key.
*Note: Public instances may rate-limit you if you search too fast.*

*   **Name:** `Public Web`
*   **API URL:** `https://searx.be/search?q={q}&format=json`
*   **Headers:** `{}`
*   **Results Path:** `results`
*   **Title Path:** `title`
*   **URL Path:** `url`
*   **Content Path:** `content`

---

### How to Enter These

1.  Run your app (`./bplus-searchrs`).
2.  Open `http://localhost:3001`.
3.  Click the **Gear Icon (⚙️)** in the bottom right to open the Settings panel.
4.  Find the **Search Sources** section and click **+ Add**.
5.  **Copy and Paste** the details from above into the form fields.
    *   *Tip:* The `{q}` in the URL is the placeholder where your search query gets injected.
6.  Click **Save**.
7.  **Important:** You will see your new provider in the list (e.g., "Apple Podcasts"). Make sure the **Checkbox** next to it is checked.
8.  Type a query (e.g., "History" or "Tech") and hit Search.

### How to verify it worked?
When the search runs, look at the chat output. If you added "TV Shows" and searched for "Breaking Bad":
1.  The citations at the bottom of the answer should list `[TV Shows] Breaking Bad`.
2.  The link should take you to `tvmaze.com`.
