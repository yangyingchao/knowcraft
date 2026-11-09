- delete file
  - core: provid api: remove file or dir, either to trash, or delete compeletey.
    * database neends to be updated properly...
  - cli: add subcommand: remove - calls core to delete file.
  - gui: call core api.

- capture:
   - gui interface.


- cli:
  - ls: list files in current directory,
    * plan format: file: title
    * json/jsonl: {"path":"real_path", "title": "real_title"}
