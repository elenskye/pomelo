-- Atlas keeps every run as history and draws each with the design it was made for. design: `<id>@<version>` of a
-- release under src/vendor/pomelo/atlas/ (atlas.rs DESIGN when the run was written); NULL = written before designs
-- had names (drawn with the first one, tabloid@1.0.0). A year done again is one more row; only the user deletes rows.
ALTER TABLE year_atlas ADD COLUMN design TEXT;
