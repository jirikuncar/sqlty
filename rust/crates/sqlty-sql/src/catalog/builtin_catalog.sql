CREATE TABLE pg_catalog.pg_namespace (
    oid oid NOT NULL, nspname name NOT NULL, nspowner oid NOT NULL, nspacl aclitem[]);
CREATE TABLE pg_catalog.pg_class (
    oid oid NOT NULL, relname name NOT NULL, relnamespace oid NOT NULL, reltype oid NOT NULL,
    reloftype oid NOT NULL, relowner oid NOT NULL, relam oid NOT NULL, relfilenode oid NOT NULL,
    reltablespace oid NOT NULL, relpages int4 NOT NULL, reltuples float4 NOT NULL,
    relallvisible int4 NOT NULL, reltoastrelid oid NOT NULL, relhasindex bool NOT NULL,
    relisshared bool NOT NULL, relpersistence "char" NOT NULL, relkind "char" NOT NULL,
    relnatts int2 NOT NULL, relchecks int2 NOT NULL, relhasrules bool NOT NULL,
    relhastriggers bool NOT NULL, relhassubclass bool NOT NULL, relrowsecurity bool NOT NULL,
    relforcerowsecurity bool NOT NULL, relispopulated bool NOT NULL,
    relreplident "char" NOT NULL, relispartition bool NOT NULL, relrewrite oid NOT NULL,
    relfrozenxid xid NOT NULL, relminmxid xid NOT NULL, relacl aclitem[], reloptions text[],
    relpartbound pg_node_tree);
CREATE TABLE pg_catalog.pg_attribute (
    attrelid oid NOT NULL, attname name NOT NULL, atttypid oid NOT NULL, attlen int2 NOT NULL,
    attnum int2 NOT NULL, attcacheoff int4 NOT NULL, atttypmod int4 NOT NULL,
    attndims int2 NOT NULL, attbyval bool NOT NULL, attalign "char" NOT NULL,
    attstorage "char" NOT NULL, attcompression "char" NOT NULL, attnotnull bool NOT NULL,
    atthasdef bool NOT NULL, atthasmissing bool NOT NULL, attidentity "char" NOT NULL,
    attgenerated "char" NOT NULL, attisdropped bool NOT NULL, attislocal bool NOT NULL,
    attinhcount int2 NOT NULL, attcollation oid NOT NULL, attstattarget int2,
    attacl aclitem[], attoptions text[], attfdwoptions text[], attmissingval anyarray);
CREATE TABLE pg_catalog.pg_type (
    oid oid NOT NULL, typname name NOT NULL, typnamespace oid NOT NULL, typowner oid NOT NULL,
    typlen int2 NOT NULL, typbyval bool NOT NULL, typtype "char" NOT NULL,
    typcategory "char" NOT NULL, typispreferred bool NOT NULL, typisdefined bool NOT NULL,
    typdelim "char" NOT NULL, typrelid oid NOT NULL, typsubscript regproc NOT NULL,
    typelem oid NOT NULL, typarray oid NOT NULL, typinput regproc NOT NULL,
    typoutput regproc NOT NULL, typreceive regproc NOT NULL, typsend regproc NOT NULL,
    typmodin regproc NOT NULL, typmodout regproc NOT NULL, typanalyze regproc NOT NULL,
    typalign "char" NOT NULL, typstorage "char" NOT NULL, typnotnull bool NOT NULL,
    typbasetype oid NOT NULL, typtypmod int4 NOT NULL, typndims int4 NOT NULL,
    typcollation oid NOT NULL, typdefaultbin pg_node_tree, typdefault text,
    typacl aclitem[]);
CREATE TABLE pg_catalog.pg_index (
    indexrelid oid NOT NULL, indrelid oid NOT NULL, indnatts int2 NOT NULL,
    indnkeyatts int2 NOT NULL, indisunique bool NOT NULL, indnullsnotdistinct bool NOT NULL,
    indisprimary bool NOT NULL, indisexclusion bool NOT NULL, indimmediate bool NOT NULL,
    indisclustered bool NOT NULL, indisvalid bool NOT NULL, indcheckxmin bool NOT NULL,
    indisready bool NOT NULL, indislive bool NOT NULL, indisreplident bool NOT NULL,
    indkey int2vector NOT NULL, indcollation oidvector NOT NULL, indclass oidvector NOT NULL,
    indoption int2vector NOT NULL, indexprs pg_node_tree, indpred pg_node_tree);
CREATE TABLE pg_catalog.pg_constraint (
    oid oid NOT NULL, conname name NOT NULL, connamespace oid NOT NULL,
    contype "char" NOT NULL, condeferrable bool NOT NULL, condeferred bool NOT NULL,
    convalidated bool NOT NULL, conrelid oid NOT NULL, contypid oid NOT NULL,
    conindid oid NOT NULL, conparentid oid NOT NULL, confrelid oid NOT NULL,
    confupdtype "char" NOT NULL, confdeltype "char" NOT NULL, confmatchtype "char" NOT NULL,
    conislocal bool NOT NULL, coninhcount int2 NOT NULL, connoinherit bool NOT NULL,
    conkey int2[], confkey int2[], conpfeqop oid[], conppeqop oid[], conffeqop oid[],
    confdelsetcols int2[], conexclop oid[], conbin pg_node_tree);
CREATE TABLE pg_catalog.pg_enum (
    oid oid NOT NULL, enumtypid oid NOT NULL, enumsortorder float4 NOT NULL,
    enumlabel name NOT NULL);
CREATE TABLE pg_catalog.pg_proc (
    oid oid NOT NULL, proname name NOT NULL, pronamespace oid NOT NULL, proowner oid NOT NULL,
    prolang oid NOT NULL, procost float4 NOT NULL, prorows float4 NOT NULL,
    provariadic oid NOT NULL, prosupport regproc NOT NULL, prokind "char" NOT NULL,
    prosecdef bool NOT NULL, proleakproof bool NOT NULL, proisstrict bool NOT NULL,
    proretset bool NOT NULL, provolatile "char" NOT NULL, proparallel "char" NOT NULL,
    pronargs int2 NOT NULL, pronargdefaults int2 NOT NULL, prorettype oid NOT NULL,
    proargtypes oidvector NOT NULL, proallargtypes oid[], proargmodes "char"[],
    proargnames text[], proargdefaults pg_node_tree, protrftypes oid[], prosrc text NOT NULL,
    probin text, prosqlbody pg_node_tree, proconfig text[], proacl aclitem[]);
CREATE TABLE pg_catalog.pg_attrdef (
    oid oid NOT NULL, adrelid oid NOT NULL, adnum int2 NOT NULL, adbin pg_node_tree NOT NULL);
CREATE TABLE pg_catalog.pg_depend (
    classid oid NOT NULL, objid oid NOT NULL, objsubid int4 NOT NULL, refclassid oid NOT NULL,
    refobjid oid NOT NULL, refobjsubid int4 NOT NULL, deptype "char" NOT NULL);
CREATE TABLE pg_catalog.pg_description (
    objoid oid NOT NULL, classoid oid NOT NULL, objsubid int4 NOT NULL,
    description text NOT NULL);
CREATE TABLE pg_catalog.pg_database (
    oid oid NOT NULL, datname name NOT NULL, datdba oid NOT NULL, encoding int4 NOT NULL,
    datlocprovider "char" NOT NULL, datistemplate bool NOT NULL, datallowconn bool NOT NULL,
    dathasloginevt bool NOT NULL, datconnlimit int4 NOT NULL, datfrozenxid xid NOT NULL,
    datminmxid xid NOT NULL, dattablespace oid NOT NULL, datcollate text NOT NULL,
    datctype text NOT NULL, datlocale text, daticurules text, datcollversion text,
    datacl aclitem[]);
CREATE TABLE pg_catalog.pg_stats (
    schemaname name, tablename name, attname name, inherited bool, null_frac float4,
    avg_width int4, n_distinct float4, most_common_vals anyarray, most_common_freqs float4[],
    histogram_bounds anyarray, correlation float4, most_common_elems anyarray,
    most_common_elem_freqs float4[], elem_count_histogram float4[],
    range_length_histogram anyarray, range_empty_frac float4,
    range_bounds_histogram anyarray);
CREATE TABLE pg_catalog.pg_roles (
    rolname name, rolsuper bool, rolinherit bool,
    rolcreaterole bool, rolcreatedb bool, rolcanlogin bool,
    rolreplication bool, rolconnlimit int4, rolpassword text,
    rolvaliduntil timestamptz, rolbypassrls bool, rolconfig text[],
    oid oid);
CREATE TABLE pg_catalog.pg_settings (
    name text, setting text, unit text, category text, short_desc text,
    extra_desc text, context text, vartype text, source text,
    min_val text, max_val text, enumvals text[], boot_val text, reset_val text,
    sourcefile text, sourceline int4, pending_restart bool);
CREATE TABLE pg_catalog.pg_stat_activity (
    datid oid, datname name, pid int4, leader_pid int4, usesysid oid, usename name,
    application_name text, client_addr inet, client_hostname text, client_port int4,
    backend_start timestamptz, xact_start timestamptz, query_start timestamptz,
    state_change timestamptz, wait_event_type text, wait_event text, state text,
    backend_xid xid, backend_xmin xid, query_id int8, query text, backend_type text);
CREATE TABLE pg_catalog.pg_tables (
    schemaname name, tablename name, tableowner name, tablespace name,
    hasindexes bool, hasrules bool, hastriggers bool,
    rowsecurity bool);
CREATE TABLE pg_catalog.pg_views (
    schemaname name, viewname name, viewowner name, definition text);
CREATE TABLE pg_catalog.pg_indexes (
    schemaname name, tablename name, indexname name, tablespace name,
    indexdef text);
CREATE TABLE pg_catalog.pg_sequences (
    schemaname name, sequencename name, sequenceowner name,
    data_type regtype, start_value int8, min_value int8,
    max_value int8, increment_by int8, cycle bool,
    cache_size int8, last_value int8);
CREATE TABLE information_schema.schemata (
    catalog_name name, schema_name name, schema_owner name,
    default_character_set_catalog name, default_character_set_schema name,
    default_character_set_name name, sql_path varchar);
CREATE TABLE information_schema.tables (
    table_catalog name, table_schema name, table_name name, table_type varchar,
    self_referencing_column_name name, reference_generation varchar,
    user_defined_type_catalog name, user_defined_type_schema name,
    user_defined_type_name name, is_insertable_into varchar, is_typed varchar,
    commit_action varchar);
CREATE TABLE information_schema.views (
    table_catalog name, table_schema name, table_name name, view_definition varchar,
    check_option varchar, is_updatable varchar, is_insertable_into varchar,
    is_trigger_updatable varchar, is_trigger_deletable varchar,
    is_trigger_insertable_into varchar);
CREATE TABLE information_schema.columns (
    table_catalog name, table_schema name, table_name name, column_name name,
    ordinal_position int4, column_default varchar, is_nullable varchar, data_type varchar,
    character_maximum_length int4, character_octet_length int4, numeric_precision int4,
    numeric_precision_radix int4, numeric_scale int4, datetime_precision int4,
    interval_type varchar, interval_precision int4, character_set_catalog name,
    character_set_schema name, character_set_name name, collation_catalog name,
    collation_schema name, collation_name name, domain_catalog name, domain_schema name,
    domain_name name, udt_catalog name, udt_schema name, udt_name name, scope_catalog name,
    scope_schema name, scope_name name, maximum_cardinality int4, dtd_identifier name,
    is_self_referencing varchar, is_identity varchar, identity_generation varchar,
    identity_start varchar, identity_increment varchar, identity_maximum varchar,
    identity_minimum varchar, identity_cycle varchar, is_generated varchar,
    generation_expression varchar, is_updatable varchar);
CREATE TABLE information_schema.routines (
    specific_catalog name, specific_schema name, specific_name name, routine_catalog name,
    routine_schema name, routine_name name, routine_type varchar, module_catalog name,
    module_schema name, module_name name, udt_catalog name, udt_schema name, udt_name name,
    data_type varchar, character_maximum_length int4, character_octet_length int4,
    character_set_catalog name, character_set_schema name, character_set_name name,
    collation_catalog name, collation_schema name, collation_name name,
    numeric_precision int4, numeric_precision_radix int4, numeric_scale int4,
    datetime_precision int4, interval_type varchar, interval_precision int4,
    type_udt_catalog name, type_udt_schema name, type_udt_name name, scope_catalog name,
    scope_schema name, scope_name name, maximum_cardinality int4, dtd_identifier name,
    routine_body varchar, routine_definition varchar, external_name varchar,
    external_language varchar, parameter_style varchar, is_deterministic varchar,
    sql_data_access varchar, is_null_call varchar, sql_path varchar,
    schema_level_routine varchar, max_dynamic_result_sets int4, is_user_defined_cast varchar,
    is_implicitly_invocable varchar, security_type varchar, to_sql_specific_catalog name,
    to_sql_specific_schema name, to_sql_specific_name name, as_locator varchar,
    created timestamptz, last_altered timestamptz, new_savepoint_level varchar,
    is_udt_dependent varchar, result_cast_from_data_type varchar,
    result_cast_as_locator varchar, result_cast_char_max_length int4,
    result_cast_char_octet_length int4, result_cast_char_set_catalog name,
    result_cast_char_set_schema name, result_cast_char_set_name name,
    result_cast_collation_catalog name, result_cast_collation_schema name,
    result_cast_collation_name name, result_cast_numeric_precision int4,
    result_cast_numeric_precision_radix int4, result_cast_numeric_scale int4,
    result_cast_datetime_precision int4, result_cast_interval_type varchar,
    result_cast_interval_precision int4, result_cast_type_udt_catalog name,
    result_cast_type_udt_schema name, result_cast_type_udt_name name,
    result_cast_scope_catalog name, result_cast_scope_schema name,
    result_cast_scope_name name, result_cast_maximum_cardinality int4,
    result_cast_dtd_identifier name);
CREATE TABLE information_schema.key_column_usage (
    constraint_catalog name, constraint_schema name, constraint_name name,
    table_catalog name, table_schema name, table_name name, column_name name,
    ordinal_position int4, position_in_unique_constraint int4);
CREATE TABLE information_schema.constraint_column_usage (
    table_catalog name, table_schema name, table_name name, column_name name,
    constraint_catalog name, constraint_schema name, constraint_name name);
CREATE TABLE information_schema.table_constraints (
    constraint_catalog name, constraint_schema name, constraint_name name,
    table_catalog name, table_schema name, table_name name, constraint_type varchar,
    is_deferrable varchar, initially_deferred varchar, enforced varchar,
    nulls_distinct varchar);
