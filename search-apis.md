### The Concept: JSON is a Tree 🌳

APIs return data in **JSON** format. Think of it like a file folder system.

* `{ }` (Curly braces) are **Folders** (Objects). They contain named keys.
* `[ ]` (Square brackets) are **Lists** (Arrays). They contain multiple items.

Your goal is to tell the app:

1. **Results Path:** "Which folder holds the list of results?"
2. **Item Paths:** "Inside one result, what are the filenames for Title, URL, and Content?"

---

### Step 1: Get the Raw Data

Open your web browser. You don't need code to do this.

Let's say you want to add **Apple Podcasts**.

1. Search Google for "Apple Itunes Search API".
2. You find a URL example: `https://itunes.apple.com/search?term=jack+johnson&entity=podcast`.
3. Paste that URL into your browser address bar.

You will see text that looks like this (I have formatted it to make it readable):

```json
{
  "resultCount": 50,
  "results": [           <-- THIS IS YOUR LIST!
    {
      "wrapperType": "track",
      "kind": "podcast",
      "collectionName": "The Jack Johnson Show",   <-- THIS LOOKS LIKE A TITLE
      "collectionViewUrl": "https://itunes.apple...", <-- THIS IS THE URL
      "artistName": "Jack Johnson",  <-- THIS COULD BE CONTENT
      "releaseDate": "2024-01-01"
    },
    ... (more items)
  ]
}
```

### Step 2: Map the "Results Path"

Look for the square bracket `[` that contains the actual data.
In the Apple example above, the `[` is inside a key named `"results"`.

* **Results Path:** `results`

*(If the list started deeper, like `"data": { "items": [...] }`, the path would be `data.items`)*.

### Step 3: Map the Item Paths

Now look at **just one item** inside that list (between the `{ }`). Pick the fields you want.

* **Title Path:** `collectionName` (This holds the name of the podcast).
* **URL Path:** `collectionViewUrl` (This holds the link).
* **Content Path:** `artistName` (This is the text description you want the LLM to read).

---

### Advanced Example: Reddit (Deep Nesting)

Reddit is tricky. Let's look at `https://www.reddit.com/r/technology.json?limit=2`.

```json
{
  "kind": "Listing",
  "data": {                  <-- Folder "data"
    "children": [            <-- Folder "children" (This holds the list!)
      {
        "kind": "t3",
        "data": {            <-- Inside the item, there is ANOTHER "data" folder
          "title": "AI is taking over",
          "url": "https://...",
          "selftext": "Here is the article body..."
        }
      }
    ]
  }
}
```

**How to map this:**

1. **Results Path:** To get to the `[ ]`, we go into `data`, then `children`.
   * Value: `data.children`
2. **Item Paths:** Now imagine you are standing *inside* one of those children. To get the title, you have to enter the item's *own* `data` folder first.
   * Title: `data.title`
   * URL: `data.url`
   * Content: `data.selftext`

---

### Edge Case: The "Root Array" (TVMaze)

Some APIs don't have a wrapper folder. They just give you the list immediately.
Example: `https://api.tvmaze.com/search/shows?q=breaking+bad`

```json
[     <-- The list starts immediately! No "results" or "data" key.
  {
    "score": 0.9,
    "show": {
      "name": "Breaking Bad",
      "url": "https://www.tvmaze.com/shows/169/breaking-bad",
      "summary": "A high school chemistry teacher..."
    }
  }
]
```

**How to map this:**

1. **Results Path:** Since it's at the very top, you leave it **Empty** (or just don't type anything in the config). The code handles empty paths as "the root".
2. **Item Paths:** Inside an item, the info is tucked inside a `show` folder.
   * Title: `show.name`
   * URL: `show.url`
   * Content: `show.summary`

---

### Summary Checklist for Finding APIs

1. **Find a URL:** Google "[Service Name] API json example".
2. **Test in Browser:** Paste the URL. (If the JSON looks like a messy wall of text, use a site like [jsonformatter.org](https://jsonformatter.org) to paste it in and make it readable).
3. **Find the List:** Look for the `[` bracket. Trace the keys (folders) needed to reach it. That's your **Results Path**.
4. **Find the Data:** Look inside one item. Trace the keys to get the text you want. These are your **Title/URL/Content Paths**.
5. **Configure:** In your app, set the URL and replace the search term with `{q}`.

**Pro Tip:** If an API requires a Key (like "Authorization: Bearer 12345"), you put that in the **Headers** field in your settings as `{"Authorization": "Bearer 12345"}`.




### *NOTE Some APIs require keywords. In this example assume you searched for the query below in the UI.

The issue is the **Search Query**.

*   **You searched:** `podcasts related to andrew santino`
*   **iTunes received:** `term=podcasts related to andrew santino`

**The Difference:**
*   **DuckDuckGo (Native)** is a "Semantic Search Engine." It understands natural language. It knows "related to" is a connector, not a name.
*   **Apple/Generic APIs** are usually "Keyword Databases." They are dumb. They look for a podcast explicitly named "related" or "to". Since it couldn't match that exact phrase, it returned whatever loose matches it could find (or garbage), pushing "Whiskey Ginger" off the list.

**The Fix:**
When using direct database APIs (like Apple, TVMaze, etc.), **search using keywords only**.
*   Try searching: `Andrew Santino`
*   Result: You will see "Whiskey Ginger" immediately.

---

### Debugger included for user(generic) APIs. How to use this Output to find NEW APIs

Your debug output gives you the answer directly!

Look at the line `Available Keys in first result:` in your log. This is the "Cheat Sheet" the server gives you for any API you add.

**Example Scenario:**
Imagine you add a **Book Search API**.
1.  You run a search.
2.  The terminal prints:
    `Available Keys: ["vol_title", "author_name", "pdf_link", "cover_img"]`

Now you know exactly what to type in the Settings UI:
*   **Title Path:** `vol_title` (because you saw it in the log)
*   **Content Path:** `author_name`
*   **URL Path:** `pdf_link`

You don't need to guess. Just add the API, run a search, look at the terminal, and update the settings with the keys you see!
