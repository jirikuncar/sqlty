CREATE TABLE bookings (
    id INTEGER PRIMARY KEY,
    room TEXT NOT NULL,
    during TSTZRANGE NOT NULL,
    nights DATERANGE
);
