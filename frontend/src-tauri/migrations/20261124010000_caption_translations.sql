CREATE TABLE caption_translations (
    source_lang TEXT NOT NULL,
    source_text TEXT NOT NULL,
    english TEXT NOT NULL,
    PRIMARY KEY (source_lang, source_text)
);
