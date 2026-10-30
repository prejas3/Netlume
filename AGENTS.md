CCS/1
P:Netlume
PURPOSE:Windows per-app network monitor and limiter
RULE:ponytail; minimal code, files, dependencies
RULE:targeted tests; one full run at card end
RULE:UI=custom controls; no default browser/OS controls
RULE:engine owns src-tauri/src/engine; UI owns src
RULE:contract=src-tauri/src/model.rs plus its Tauri commands/events
RULE:contract changes only on its owning card and are recorded on that card
