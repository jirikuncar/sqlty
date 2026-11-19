class Participant:
    def name(self) -> str:
        return self.db.one("SELECT nme FROM users WHERE id = %s", (self.id,))


def first(cursor, other):
    cursor.one("SELECT idd FROM users")
    other.one("SELECT nope FROM users")
