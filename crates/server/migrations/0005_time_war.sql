-- NULL retains the standard, open-ended starting position for existing rooms.
ALTER TABLE games ADD COLUMN time_war_separation INTEGER
    CHECK (time_war_separation BETWEEN 3 AND 255 AND time_war_separation % 2 = 1);
