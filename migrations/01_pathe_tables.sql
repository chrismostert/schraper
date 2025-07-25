CREATE TABLE cities (
    slug TEXT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE cinemas (
    slug TEXT PRIMARY KEY,
    city_slug TEXT REFERENCES cities (slug),
    name TEXT NOT NULL
);

CREATE TABLE shows (
    slug TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    release_at TEXT,
    movie_type TEXT NOT NULL,
    duration INTEGER NOT NULL
);

CREATE TABLE posters (
    show_slug TEXT PRIMARY KEY REFERENCES shows (slug),
    lg TEXT,
    md TEXT
);

CREATE TABLE genres (
    show_slug TEXT NOT NULL REFERENCES shows (slug),
    genre TEXT NOT NULL,
    PRIMARY KEY(show_slug, genre)
);

CREATE TABLE showtimes (
    show_slug TEXT REFERENCES shows (slug),
    cinema_slug TEXT REFERENCES cinemas (slug),
    time TEXT,
    reservation_url TEXT,
    auditorium_name TEXT,
    auditorium_capacity TEXT,
    end_time TEXT,
    PRIMARY KEY(show_slug, cinema_slug, time, auditorium_name)
);