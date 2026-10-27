-- CREATE FUNCTION pairs: each statement with planner hints is followed by the same
-- text without them. Generated with names that collide with the hints; PostgreSQL
-- accepts both. Statements end with `;` and a newline and contain no other `;`.

create function support() Returns Setof int  language sql AS $f$ SELECT security volatile volatile $f$ Rows	+5;
create function support() Returns Setof int  language sql AS $f$ SELECT security volatile volatile $f$;

Create Function public.stable() returns "rows" language Sql -- c
 leakproof AS $f$ SELECT cost leakproof definer $f$;
Create Function public.stable() returns "rows" language Sql AS $f$ SELECT cost leakproof definer $f$;

Create FUNCTION public.cost() RETURNS setof rows.support
    AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Language Sql ROWS 1;
Create FUNCTION public.cost() RETURNS setof rows.support
    AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Language Sql;

CREATE FUNCTION "rows"(current "rows") Returns setof bigint Language SQL rows	5. AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
CREATE FUNCTION "rows"(current "rows") Returns setof bigint Language SQL AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

CREATE FUNCTION x()  returns bigint
    AS $$ SELECT NULL /* atomic input returns */ $$
    Not  leakproof LANGUAGE sql;
CREATE FUNCTION x()  returns bigint
    AS $$ SELECT NULL /* atomic input returns */ $$ LANGUAGE sql;

CREATE Function public."returns"()	returns SETOF integer  LANGUAGE SQL	AS $f$ SELECT setof stable parallel $f$	leakproof;
CREATE Function public."returns"()	returns SETOF integer  LANGUAGE SQL	AS $f$ SELECT setof stable parallel $f$;

Create Function current() RETURNS Setof support.cost -- c
 LEAKPROOF  language sql  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
Create Function current() RETURNS Setof support.cost  language sql  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

CREATE Function "security"()  RETURNS int[]  AS 'SELECT ''it''''s strict returns language''' STRICT LEAKPROOF	language SQL;
CREATE Function "security"()  RETURNS int[]  AS 'SELECT ''it''''s strict returns language''' STRICT	language SQL;

Create FUNCTION public.tickets() -- c
 returns setof rows  cost  +5
    language sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
Create FUNCTION public.tickets() -- c
 returns setof rows
    language sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create function language() returns rows	Language SQL -- c
 LEAKPROOF
    AS $$ SELECT NULL /* leakproof setof stable */ $$;
Create function language() returns rows	Language SQL
    AS $$ SELECT NULL /* leakproof setof stable */ $$;

Create FUNCTION leakproof() RETURNS SETOF boolean leakproof -- c
 AS $f$ SELECT transform language stable $f$  language sql;
Create FUNCTION leakproof() RETURNS SETOF boolean -- c
 AS $f$ SELECT transform language stable $f$  language sql;

create function transform() -- c
 Returns SETOF support[] LANGUAGE sql  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$	not Leakproof;
create function transform() -- c
 Returns SETOF support[] LANGUAGE sql  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create Function "g"() returns boolean -- c
 PARALLEL UNSAFE COST  1e+06	LANGUAGE Sql  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
Create Function "g"() returns boolean -- c
 PARALLEL UNSAFE	LANGUAGE Sql  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create function stable()	Returns support.cost -- c
 LANGUAGE sql  Parallel Safe Leakproof AS $f$ SELECT safe safe strict $f$;
Create function stable()	Returns support.cost -- c
 LANGUAGE sql  Parallel Safe AS $f$ SELECT safe safe strict $f$;

create OR REPLACE FUNCTION external()	RETURNS int[]  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ -- c
 COST
    1_000  LANGUAGE Sql;
create OR REPLACE FUNCTION external()	RETURNS int[]  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$  LANGUAGE Sql;

Create FUNCTION atomic(x cost[]) /* c */ returns text /* c */ Cost 0.5	LANGUAGE sql AS $f$ SELECT parallel input security $f$;
Create FUNCTION atomic(x cost[]) /* c */ returns text	LANGUAGE sql AS $f$ SELECT parallel input security $f$;

Create FUNCTION rows.stable() /* c */ returns text COST
    2.5e+06 -- c
 language SQL  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
Create FUNCTION rows.stable() /* c */ returns text -- c
 language SQL  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create function "cost".cost(called uuid) returns support[] LANGUAGE sql  Cost -- c
 10  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
Create function "cost".cost(called uuid) returns support[] LANGUAGE sql  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

CREATE Function volatile()  returns Setof cost language sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Strict	LEAKPROOF  Immutable;
CREATE Function volatile()  returns Setof cost language sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Strict  Immutable;

Create FUNCTION returns() RETURNS numeric(10, 2)  AS 'SELECT ''it''''s cost parallel security'''  LANGUAGE sql -- c
 leakproof;
Create FUNCTION returns() RETURNS numeric(10, 2)  AS 'SELECT ''it''''s cost parallel security'''  LANGUAGE sql;

Create FUNCTION public.setof() RETURNS SETOF cost PARALLEL Safe LANGUAGE SQL AS $f$ SELECT safe setof parallel $f$  Rows	1.5E2;
Create FUNCTION public.setof() RETURNS SETOF cost PARALLEL Safe LANGUAGE SQL AS $f$ SELECT safe setof parallel $f$;

Create function "window"() RETURNS setof cost[] Cost 100.0 VOLATILE -- c
 Language Sql AS $f$ SELECT window definer called $f$;
Create function "window"() RETURNS setof cost[] VOLATILE -- c
 Language Sql AS $f$ SELECT window definer called $f$;

CREATE FUNCTION strict(current support, safe leakproof.rows) returns boolean  LANGUAGE Sql not leakproof RETURN (safe IS NULL);
CREATE FUNCTION strict(current support, safe leakproof.rows) returns boolean  LANGUAGE Sql RETURN (safe IS NULL);

CREATE OR REPLACE Function public."cost"()
    RETURNS cost[] AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Not leakproof LANGUAGE SQL;
CREATE OR REPLACE Function public."cost"()
    RETURNS cost[] AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ LANGUAGE SQL;

CREATE FUNCTION definer() -- c
 RETURNS SETOF boolean Rows	0.5 AS 'SELECT ''it''''s called leakproof stable'''
    language SQL;
CREATE FUNCTION definer() -- c
 RETURNS SETOF boolean AS 'SELECT ''it''''s called leakproof stable'''
    language SQL;

create function public.current()	Returns public . support leakproof AS $f$ SELECT current parallel returns $f$
    Language Sql;
create function public.current()	Returns public . support AS $f$ SELECT current parallel returns $f$
    Language Sql;

Create FUNCTION atomic()	returns SETOF leakproof
    AS $f$ SELECT immutable stable volatile $f$ Stable  LANGUAGE Sql ROWS 3e-1;
Create FUNCTION atomic()	returns SETOF leakproof
    AS $f$ SELECT immutable stable volatile $f$ Stable  LANGUAGE Sql;

CREATE function rows() returns setof text Strict AS 'SELECT ''it''''s immutable input language''' Rows 1_000 -- c
 LANGUAGE sql;
CREATE function rows() returns setof text Strict AS 'SELECT ''it''''s immutable input language''' -- c
 LANGUAGE sql;

Create Function "x"() Returns SETOF cost	Cost 1_000	set search_path TO public AS $f$ SELECT rows volatile setof $f$	Language SQL;
Create Function "x"() Returns SETOF cost	set search_path TO public AS $f$ SELECT rows volatile setof $f$	Language SQL;

create Function security() RETURNS cost Parallel unsafe AS 'SELECT ''it''''s support parallel strict''' Not	leakproof LANGUAGE SQL;
create Function security() RETURNS cost Parallel unsafe AS 'SELECT ''it''''s support parallel strict''' LANGUAGE SQL;

Create OR REPLACE function "strict"()	Returns bigint	AS $$ SELECT NULL /* called stable strict */ $$ cost 2.5e+06
    LANGUAGE Sql;
Create OR REPLACE function "strict"()	Returns bigint	AS $$ SELECT NULL /* called stable strict */ $$
    LANGUAGE Sql;

create FUNCTION strict() /* c */ returns public.support cost
    1.5E2 AS 'SELECT ''it''''s safe definer volatile'''  Language sql;
create FUNCTION strict() /* c */ returns public.support AS 'SELECT ''it''''s safe definer volatile'''  Language sql;

Create Function current() Returns boolean	volatile AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Language Sql
    Cost 1e3 -- c
 Leakproof;
Create Function current() Returns boolean	volatile AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Language Sql;

create OR REPLACE FUNCTION strict() returns integer
    Leakproof Language Sql /* c */ Stable AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
create OR REPLACE FUNCTION strict() returns integer Language Sql /* c */ Stable AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

CREATE OR REPLACE Function x()  Returns cost  COST 1000  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ /* c */ LANGUAGE SQL SET my.safe = x;
CREATE OR REPLACE Function x()  Returns cost  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ /* c */ LANGUAGE SQL SET my.safe = x;

create OR REPLACE function leakproof.tickets() Returns setof cost LANGUAGE Sql /* c */ COST 1	AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
create OR REPLACE function leakproof.tickets() Returns setof cost LANGUAGE Sql	AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

create FUNCTION public.immutable() returns support
    AS $$ SELECT NULL /* language current transform */ $$  language Sql  Cost 0.5;
create FUNCTION public.immutable() returns support
    AS $$ SELECT NULL /* language current transform */ $$  language Sql;

Create OR REPLACE Function public.f()  returns Setof rows.support Language Sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$	Cost 5.  strict;
Create OR REPLACE Function public.f()  returns Setof rows.support Language Sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$  strict;

create Function returns()	returns int cost -- c
 1_000 LANGUAGE sql	SET cost.x TO public AS 'SELECT ''it''''s parallel atomic safe''';
create Function returns()	returns int LANGUAGE sql	SET cost.x TO public AS 'SELECT ''it''''s parallel atomic safe''';

create FUNCTION current(input support[])	returns support AS 'SELECT ''it''''s strict security atomic''' Language Sql Cost -- c
 1_000;
create FUNCTION current(input support[])	returns support AS 'SELECT ''it''''s strict security atomic''' Language Sql;

Create Function leakproof() returns cost	AS $$ SELECT NULL /* current volatile called */ $$ LANGUAGE sql -- c
 COST .5 -- c
 volatile;
Create Function leakproof() returns cost	AS $$ SELECT NULL /* current volatile called */ $$ LANGUAGE sql -- c
 volatile;

CREATE Function public.security()	RETURNS bigint	LANGUAGE SQL AS $f$ SELECT called cost cost $f$ -- c
 Not LEAKPROOF	SECURITY DEFINER;
CREATE Function public.security()	RETURNS bigint	LANGUAGE SQL AS $f$ SELECT called cost cost $f$	SECURITY DEFINER;

create Function public.f()
    returns boolean LANGUAGE Sql  AS 'SELECT ''it''''s support external atomic''' COST 1.5E2 -- c
 VOLATILE;
create Function public.f()
    returns boolean LANGUAGE Sql  AS 'SELECT ''it''''s support external atomic''' -- c
 VOLATILE;

create FUNCTION ids()	returns rows.support	LANGUAGE Sql AS 'SELECT ''it''''s returns setof atomic''' -- c
 Cost	.5	CALLED ON NULL INPUT;
create FUNCTION ids()	returns rows.support	LANGUAGE Sql AS 'SELECT ''it''''s returns setof atomic'''	CALLED ON NULL INPUT;

Create FUNCTION parallel() /* c */ RETURNS support  LEAKPROOF AS $f$ SELECT transform stable parallel $f$ language Sql SECURITY DEFINER;
Create FUNCTION parallel() /* c */ RETURNS support AS $f$ SELECT transform stable parallel $f$ language Sql SECURITY DEFINER;

create function "current"(cost leakproof, language cost) Returns boolean  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Cost 1e+06	LANGUAGE Sql;
create function "current"(cost leakproof, language cost) Returns boolean  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$	LANGUAGE Sql;

create FUNCTION rows() RETURNS bigint SECURITY DEFINER  COST	2.5e+06	AS $f$ SELECT returns external atomic $f$  Language SQL
    stable;
create FUNCTION rows() RETURNS bigint SECURITY DEFINER	AS $f$ SELECT returns external atomic $f$  Language SQL
    stable;

create Function "cost".window() RETURNS boolean AS $f$ SELECT transform atomic input $f$  COST 1e+06  PARALLEL RESTRICTED  LANGUAGE Sql;
create Function "cost".window() RETURNS boolean AS $f$ SELECT transform atomic input $f$  PARALLEL RESTRICTED  LANGUAGE Sql;

create FUNCTION external() RETURNS Setof leakproof AS $$ SELECT NULL /* support transform external */ $$ /* c */ Leakproof language sql;
create FUNCTION external() RETURNS Setof leakproof AS $$ SELECT NULL /* support transform external */ $$ language sql;

create FUNCTION public."tickets"() Returns SETOF integer -- c
 language sql	AS $$ SELECT NULL /* atomic immutable parallel */ $$ COST	5.;
create FUNCTION public."tickets"() Returns SETOF integer -- c
 language sql	AS $$ SELECT NULL /* atomic immutable parallel */ $$;

Create FUNCTION ids() returns support[] /* c */ Cost -- c
 5. /* c */ AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$	SECURITY DEFINER LANGUAGE sql;
Create FUNCTION ids() returns support[] /* c */ AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$	SECURITY DEFINER LANGUAGE sql;

create FUNCTION public."leakproof"() returns setof boolean	SECURITY DEFINER AS $f$ SELECT cost stable rows $f$ -- c
 Cost	5. language SQL;
create FUNCTION public."leakproof"() returns setof boolean	SECURITY DEFINER AS $f$ SELECT cost stable rows $f$ language SQL;

CREATE OR REPLACE function definer(cost boolean) -- c
 Returns setof "rows"	Language SQL  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Leakproof;
CREATE OR REPLACE function definer(cost boolean) -- c
 Returns setof "rows"	Language SQL  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create OR REPLACE FUNCTION f(acc int)	Returns leakproof AS 'SELECT ''it''''s rows atomic leakproof''' -- c
 language Sql Cost 3e-1 STABLE;
Create OR REPLACE FUNCTION f(acc int)	Returns leakproof AS 'SELECT ''it''''s rows atomic leakproof''' -- c
 language Sql STABLE;

CREATE Function public.leakproof() Returns boolean COST	10 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$  RETURNS NULL ON NULL INPUT LANGUAGE SQL;
CREATE Function public.leakproof() Returns boolean AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$  RETURNS NULL ON NULL INPUT LANGUAGE SQL;

Create function immutable()
    returns setof "rows" AS $$ SELECT NULL /* rows input input */ $$
    LANGUAGE SQL immutable	Rows
    3e-1;
Create function immutable()
    returns setof "rows" AS $$ SELECT NULL /* rows input input */ $$
    LANGUAGE SQL immutable;

CREATE OR REPLACE FUNCTION public.f()	Returns SETOF rows	LEAKPROOF language Sql	AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ -- c
 COST	.5 Stable;
CREATE OR REPLACE FUNCTION public.f()	Returns SETOF rows language Sql	AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Stable;

create function public.stable(g boolean) Returns setof rows volatile AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ -- c
 ROWS  5.
    language sql;
create function public.stable(g boolean) Returns setof rows volatile AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$
    language sql;

Create function public."f"()
    returns SETOF leakproof.rows /* c */ LEAKPROOF AS $f$ SELECT volatile returns window $f$
    language sql;
Create function public."f"()
    returns SETOF leakproof.rows AS $f$ SELECT volatile returns window $f$
    language sql;

Create Function public.window() Returns TABLE ("rows" cost[]) strict	volatile Language Sql  COST 0.5 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
Create Function public.window() Returns TABLE ("rows" cost[]) strict	volatile Language Sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create FUNCTION "parallel"() Returns cost[]
    SET leakproof.x TO '$user'  Language Sql COST 1_000 AS $f$ SELECT input returns called $f$;
Create FUNCTION "parallel"() Returns cost[]
    SET leakproof.x TO '$user'  Language Sql AS $f$ SELECT input returns called $f$;

create FUNCTION "safe"() Returns uuid	Leakproof VOLATILE SET work_mem FROM CURRENT -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ language sql;
create FUNCTION "safe"() Returns uuid VOLATILE SET work_mem FROM CURRENT -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ language sql;

CREATE Function "support".f()
    returns numeric(10, 2)  language sql -- c
 strict AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ /* c */ leakproof;
CREATE Function "support".f()
    returns numeric(10, 2)  language sql -- c
 strict AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create function safe() /* c */ returns boolean /* c */ Language Sql /* c */ AS $f$ SELECT called stable safe $f$ /* c */ Immutable cost  .5;
Create function safe() /* c */ returns boolean /* c */ Language Sql /* c */ AS $f$ SELECT called stable safe $f$ /* c */ Immutable;

create OR REPLACE Function public."rows"() RETURNS Setof cost /* c */ AS 'SELECT ''it''''s returns stable current''' COST	1.5E2	language Sql;
create OR REPLACE Function public."rows"() RETURNS Setof cost /* c */ AS 'SELECT ''it''''s returns stable current'''	language Sql;

Create OR REPLACE function atomic() Returns text  Language sql cost -- c
 3e-1
    IMMUTABLE AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ LEAKPROOF;
Create OR REPLACE function atomic() Returns text  Language sql
    IMMUTABLE AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

create FUNCTION g()  returns numeric(10, 2) AS $$ SELECT NULL /* cost setof stable */ $$ STABLE  support	textlike_support -- c
 language SQL;
create FUNCTION g()  returns numeric(10, 2) AS $$ SELECT NULL /* cost setof stable */ $$ STABLE -- c
 language SQL;

CREATE FUNCTION safe() Returns bigint  LANGUAGE Sql /* c */ AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ SET search_path to 'strict'  Cost -- c
 10;
CREATE FUNCTION safe() Returns bigint  LANGUAGE Sql /* c */ AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ SET search_path to 'strict';

Create function public.cost()	Returns Setof rows -- c
 LANGUAGE Sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ support pg_catalog.textlike_support;
Create function public.cost()	Returns Setof rows -- c
 LANGUAGE Sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

create OR REPLACE FUNCTION f() Returns uuid /* c */ AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$
    language sql -- c
 Immutable cost  .5 Leakproof;
create OR REPLACE FUNCTION f() Returns uuid /* c */ AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$
    language sql -- c
 Immutable;

create FUNCTION rows(leakproof rows) Returns setof numeric(10, 2) AS 'SELECT ''it''''s safe external immutable''' NOT	Leakproof  language SQL;
create FUNCTION rows(leakproof rows) Returns setof numeric(10, 2) AS 'SELECT ''it''''s safe external immutable'''  language SQL;

CREATE OR REPLACE Function "rows".x(leakproof public . support) /* c */ Returns boolean LANGUAGE sql  cost 1.5E2 RETURN leakproof IS NOT NULL;
CREATE OR REPLACE Function "rows".x(leakproof public . support) /* c */ Returns boolean LANGUAGE sql RETURN leakproof IS NOT NULL;

CREATE FUNCTION g() Returns leakproof AS $$ SELECT NULL /* window external atomic */ $$ /* c */ SECURITY DEFINER	cost 10 /* c */ language sql;
CREATE FUNCTION g() Returns leakproof AS $$ SELECT NULL /* window external atomic */ $$ /* c */ SECURITY DEFINER /* c */ language sql;

create OR REPLACE function support.rows() RETURNS setof cost[] SUPPORT
    textlike_support	language SQL AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
create OR REPLACE function support.rows() RETURNS setof cost[]	language SQL AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create function acc() RETURNS numeric(10, 2) AS $f$ SELECT definer immutable strict $f$ -- c
 Support PG_CATALOG.TEXTLIKE_SUPPORT	LANGUAGE Sql;
Create function acc() RETURNS numeric(10, 2) AS $f$ SELECT definer immutable strict $f$	LANGUAGE Sql;

CREATE FUNCTION g() Returns Setof integer	Language SQL  Rows 2.5e+06 AS $$ SELECT NULL /* rows external strict */ $$ RETURNS NULL ON NULL INPUT;
CREATE FUNCTION g() Returns Setof integer	Language SQL AS $$ SELECT NULL /* rows external strict */ $$ RETURNS NULL ON NULL INPUT;

create FUNCTION security()  returns setof rows /* c */ LANGUAGE Sql -- c
 AS $f$ SELECT leakproof rows security $f$ ROWS 1e+06 SECURITY DEFINER;
create FUNCTION security()  returns setof rows /* c */ LANGUAGE Sql -- c
 AS $f$ SELECT leakproof rows security $f$ SECURITY DEFINER;

create FUNCTION f() RETURNS int SECURITY DEFINER Cost	0.5 Strict LANGUAGE Sql -- c
 AS $f$ SELECT input atomic input $f$ SET rows.x TO 'window';
create FUNCTION f() RETURNS int SECURITY DEFINER Strict LANGUAGE Sql -- c
 AS $f$ SELECT input atomic input $f$ SET rows.x TO 'window';

CREATE function public.returns()  RETURNS boolean SET rows.x TO public  Language SQL -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$
    cost  1000;
CREATE function public.returns()  RETURNS boolean SET rows.x TO public  Language SQL -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

CREATE OR REPLACE Function rows() Returns int
    COST 3e-1 SUPPORT textlike_support  language sql AS 'SELECT ''it''''s rows strict volatile''';
CREATE OR REPLACE Function rows() Returns int  language sql AS 'SELECT ''it''''s rows strict volatile''';

CREATE function public.support(f int) /* c */ returns numeric(10, 2)	Cost 1e+06 AS 'SELECT ''it''''s security support leakproof''' LANGUAGE sql;
CREATE function public.support(f int) /* c */ returns numeric(10, 2) AS 'SELECT ''it''''s security support leakproof''' LANGUAGE sql;

Create function support."setof"() Returns support Support  pg_catalog/* c */.varchar_support	AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$  language SQL;
Create function support."setof"() Returns support	AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$  language SQL;

CREATE function input(f public.support, language support[])  RETURNS boolean  SECURITY INVOKER Language SQL -- c
 leakproof RETURN f IS NOT NULL;
CREATE function input(f public.support, language support[])  RETURNS boolean  SECURITY INVOKER Language SQL RETURN f IS NOT NULL;

create function security() returns "rows" Language sql
    AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ -- c
 Support pg_catalog . array_unnest_support;
create function security() returns "rows" Language sql
    AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

CREATE FUNCTION public.acc() /* c */ RETURNS integer Parallel Safe  LANGUAGE sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$	cost	100.0 Not  leakproof;
CREATE FUNCTION public.acc() /* c */ RETURNS integer Parallel Safe  LANGUAGE sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create OR REPLACE Function public.support() returns rows Language SQL  SUPPORT PG_CATALOG.TEXTLIKE_SUPPORT  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
Create OR REPLACE Function public.support() returns rows Language SQL  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

create FUNCTION "transform"(leakproof integer, transform rows.support)	returns boolean  stable  Leakproof LANGUAGE SQL RETURN (leakproof IS NULL);
create FUNCTION "transform"(leakproof integer, transform rows.support)	returns boolean  stable LANGUAGE SQL RETURN (leakproof IS NULL);

create OR REPLACE function f()	returns numeric(10, 2) -- c
 AS $f$ SELECT safe current window $f$ Language Sql Stable
    STRICT leakproof Cost +5;
create OR REPLACE function f()	returns numeric(10, 2) -- c
 AS $f$ SELECT safe current window $f$ Language Sql Stable
    STRICT;

Create OR REPLACE function "definer"() Returns text	Volatile	AS $f$ SELECT transform setof transform $f$
    language sql parallel Unsafe  Cost	+5;
Create OR REPLACE function "definer"() Returns text	Volatile	AS $f$ SELECT transform setof transform $f$
    language sql parallel Unsafe;

CREATE Function "volatile"() RETURNS text
    NOT LEAKPROOF  language sql AS $$ SELECT NULL /* window security transform */ $$ SET rows.x TO public;
CREATE Function "volatile"() RETURNS text  language sql AS $$ SELECT NULL /* window security transform */ $$ SET rows.x TO public;

Create Function f()  RETURNS cost SET cost.x TO "rows" -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ CALLED ON NULL INPUT	LANGUAGE Sql  not LEAKPROOF;
Create Function f()  RETURNS cost SET cost.x TO "rows" -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ CALLED ON NULL INPUT	LANGUAGE Sql;

CREATE Function public.x() /* c */ Returns Setof character varying(10)  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$
    COST +5	STRICT /* c */ language sql;
CREATE Function public.x() /* c */ Returns Setof character varying(10)  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$	STRICT /* c */ language sql;

create Function current()  RETURNS boolean STABLE /* c */ SET work_mem FROM CURRENT -- c
 language sql  COST	5.  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
create Function current()  RETURNS boolean STABLE /* c */ SET work_mem FROM CURRENT -- c
 language sql  AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create OR REPLACE FUNCTION atomic() RETURNS rows LANGUAGE sql	AS $$ SELECT NULL /* called atomic rows */ $$ -- c
 support  PG_CATALOG.TEXTLIKE_SUPPORT;
Create OR REPLACE FUNCTION atomic() RETURNS rows LANGUAGE sql	AS $$ SELECT NULL /* called atomic rows */ $$;

Create function public.input()  Returns uuid
    AS $f$ SELECT input parallel stable $f$ Language sql cost 5. support	pg_catalog . array_unnest_support;
Create function public.input()  Returns uuid
    AS $f$ SELECT input parallel stable $f$ Language sql;

create FUNCTION support() /* c */ Returns public . support -- c
 Language SQL RETURNS NULL ON NULL INPUT	AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ Cost 1e3;
create FUNCTION support() /* c */ Returns public . support -- c
 Language SQL RETURNS NULL ON NULL INPUT	AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create OR REPLACE FUNCTION leakproof()
    Returns boolean -- c
 COST 1e3 RETURNS NULL ON NULL INPUT LANGUAGE Sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
Create OR REPLACE FUNCTION leakproof()
    Returns boolean RETURNS NULL ON NULL INPUT LANGUAGE Sql AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create Function public.rows() returns SETOF int	PARALLEL Unsafe AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ COST 1 RETURNS NULL ON NULL INPUT
    language sql;
Create Function public.rows() returns SETOF int	PARALLEL Unsafe AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ RETURNS NULL ON NULL INPUT
    language sql;

Create FUNCTION ids() Returns bigint	support  PG_CATALOG.TEXTLIKE_SUPPORT /* c */ PARALLEL Safe -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ language SQL;
Create FUNCTION ids() Returns bigint /* c */ PARALLEL Safe -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ language SQL;

CREATE function called() RETURNS support	language SQL -- c
 VOLATILE support pg_catalog/* c */.varchar_support AS 'SELECT ''it''''s support rows atomic''';
CREATE function called() RETURNS support	language SQL -- c
 VOLATILE AS 'SELECT ''it''''s support rows atomic''';

CREATE OR REPLACE function public.f() returns SETOF leakproof.rows Support  PG_CATALOG.TEXTLIKE_SUPPORT language SQL	AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
CREATE OR REPLACE function public.f() returns SETOF leakproof.rows language SQL	AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create OR REPLACE function strict(current cost[])
    RETURNS boolean -- c
 Cost -- c
 1.5E2 SET work_mem FROM CURRENT	language sql RETURN current IS NULL;
Create OR REPLACE function strict(current cost[])
    RETURNS boolean SET work_mem FROM CURRENT	language sql RETURN current IS NULL;

create function returns() Returns Setof rows -- c
 support	pg_catalog/* c */.varchar_support /* c */ language Sql
    AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
create function returns() Returns Setof rows /* c */ language Sql
    AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create Function tickets() RETURNS int[] SET search_path to public	language sql CALLED ON NULL INPUT AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ cost /* c */ 1000;
Create Function tickets() RETURNS int[] SET search_path to public	language sql CALLED ON NULL INPUT AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

create Function "x"(strict bigint, stable int[]) Returns boolean	LANGUAGE Sql -- c
 AS $f$ SELECT rows called parallel $f$	Cost 1e3 set search_path = "rows";
create Function "x"(strict bigint, stable int[]) Returns boolean	LANGUAGE Sql -- c
 AS $f$ SELECT rows called parallel $f$ set search_path = "rows";

CREATE FUNCTION rows() RETURNS SETOF rows SET work_mem FROM CURRENT /* c */ language SQL parallel UNSAFE Cost 10 /* c */ AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
CREATE FUNCTION rows() RETURNS SETOF rows SET work_mem FROM CURRENT /* c */ language SQL parallel UNSAFE /* c */ AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

create function "support"() Returns SETOF int STABLE LANGUAGE sql  AS $$ SELECT NULL /* rows safe parallel */ $$	set search_path = external, 'cost' Rows	1e+06;
create function "support"() Returns SETOF int STABLE LANGUAGE sql  AS $$ SELECT NULL /* rows safe parallel */ $$	set search_path = external, 'cost';

CREATE Function public.tickets(cost int[]) /* c */ RETURNS leakproof cost	5. Language sql -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ SET leakproof.x TO "rows";
CREATE Function public.tickets(cost int[]) /* c */ RETURNS leakproof Language sql -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ SET leakproof.x TO "rows";

create OR REPLACE FUNCTION "cost"() -- c
 Returns TABLE (returns boolean) language sql	not leakproof SET cost.x TO "rows" AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
create OR REPLACE FUNCTION "cost"() -- c
 Returns TABLE (returns boolean) language sql SET cost.x TO "rows" AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

create Function "support"."ids"() -- c
 Returns setof support[] Language Sql SUPPORT "pg_catalog"."textlike_support" -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
create Function "support"."ids"() -- c
 Returns setof support[] Language Sql -- c
 AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

Create Function leakproof()	RETURNS bigint
    leakproof AS $$ SELECT NULL /* setof language parallel */ $$ Immutable SET work_mem FROM CURRENT
    language SQL;
Create Function leakproof()	RETURNS bigint AS $$ SELECT NULL /* setof language parallel */ $$ Immutable SET work_mem FROM CURRENT
    language SQL;

Create function safe() RETURNS setof uuid LANGUAGE sql cost 5. /* c */ AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ VOLATILE  Set search_path TO "rows", 'called', "acc";
Create function safe() RETURNS setof uuid LANGUAGE sql /* c */ AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$ VOLATILE  Set search_path TO "rows", 'called', "acc";

create Function public.stable()	RETURNS cost /* c */ Support
    "pg_catalog"."textlike_support" AS $$ SELECT NULL /* security immutable returns */ $$ LANGUAGE SQL;
create Function public.stable()	RETURNS cost AS $$ SELECT NULL /* security immutable returns */ $$ LANGUAGE SQL;

Create function public.volatile()
    RETURNS setof text Language Sql	ROWS 1e+06 SUPPORT  "pg_catalog"."textlike_support"
    AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
Create function public.volatile()
    RETURNS setof text Language Sql
    AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;

CREATE OR REPLACE function public.setof()	RETURNS boolean
    language SQL set search_path TO "rows"	AS 'SELECT ''it''''s strict current language'''  COST /* c */ 0.5;
CREATE OR REPLACE function public.setof()	RETURNS boolean
    language SQL set search_path TO "rows"	AS 'SELECT ''it''''s strict current language''';

create OR REPLACE Function "safe"() /* c */ Returns int[]	Support  "pg_catalog"."textlike_support"	LANGUAGE sql parallel Safe	AS 'SELECT ''it''''s input stable setof''';
create OR REPLACE Function "safe"() /* c */ Returns int[]	LANGUAGE sql parallel Safe	AS 'SELECT ''it''''s input stable setof''';

create OR REPLACE Function public.transform()  RETURNS boolean	language sql  IMMUTABLE SUPPORT
    "pg_catalog"."textlike_support" AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
create OR REPLACE Function public.transform()  RETURNS boolean	language sql  IMMUTABLE AS $_$ SELECT 1 ORDER BY 1 ROWS 2 $_$;
