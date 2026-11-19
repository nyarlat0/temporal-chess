ALTER TABLE games ADD COLUMN lab_setup TEXT
    CHECK (octet_length(lab_setup) <= 16777216);
ALTER TABLE games ADD CONSTRAINT one_starting_setting
    CHECK (lab_setup IS NULL OR time_war_separation IS NULL);
