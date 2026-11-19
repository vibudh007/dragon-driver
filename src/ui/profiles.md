# Profile workspace

`page` creates the Main and Fn profile workspace when the model supports onboard configuration. The model's onboard-selection flag controls the multiple-profile selectors. `Draft::new` supplies the default layers and an empty advanced definition document. `Editor` manages the selected key, visible layer, file actions and onboard upload.

`update_choices` repopulates the profile selectors and `update_selection` displays the assignment for the current key. `name_dialog` creates or renames host profiles. `file_dialog` imports vendor profile or advanced-key XML and exports the current workspace. Advanced definitions and profiles are exported together with their vendor XML element names. Import also accepts standalone vendor definitions. Opening an advanced-key workspace asks before replacing the local draft. File I/O runs on the worker. `read_profile` bounds input and `write_profile` uses a temporary file followed by rename.

`save_assignment` stages the chosen mapping locally. `validate` runs the backend planner before enabling a confirmed upload. The success message distinguishes a final vendor acceptance from transmission alone. Onboard upload can replace both layers and their referenced areas. Editing assignments in memory leaves the device unchanged.

[Assignment editor](profiles/actions.md) · [Profile planner](../keyboard/bindings.md)

`application_controls` links a Main profile to an installed desktop application. `watch` consumes worker events, `linked_profile` resolves the strongest unique match, and `activate` prepares the linked Main and selected Fn layers for one automatic write. `upload_status` updates the active profile after a successful transfer and distinguishes device confirmation. Imported profile-switch assignments follow the imported indices when XML is merged into a workspace. [Desktop behavior](desktop.md)
