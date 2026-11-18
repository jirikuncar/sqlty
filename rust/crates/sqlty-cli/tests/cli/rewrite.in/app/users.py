by_email = """--sql
select id, email from users where email = %(email)s"""
dynamic = f"""--sql
select {column} from users"""
