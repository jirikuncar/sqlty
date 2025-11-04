from sqlty._generator import TypeStubGenerator


def test_no_calls(tmp_path):
    src = tmp_path / "empty.py"
    src.write_text("# no sql calls here\n")
    gen = TypeStubGenerator()
    stub_path = tmp_path / "empty.pyi"
    result = gen.generate_stub_file(src, stub_path)
    assert result == stub_path
    stub = stub_path.read_text()
    # Should not generate any Literal overloads
    assert "Literal[" not in stub
    # Fallback overload should always be present
    assert "def sql" in stub


def test_non_literal_call(tmp_path):
    src = tmp_path / "nonlit.py"
    src.write_text("""
def sql(query):
    pass
x = "SELECT * FROM t"
sql(x)
""")
    gen = TypeStubGenerator()
    stub_path = tmp_path / "nonlit.pyi"
    gen.generate_stub_file(src, stub_path)
    stub = stub_path.read_text()
    # Should not generate overload for non-literal
    assert "Literal[" not in stub
    assert "def sql" in stub  # fallback overload always present


def test_missing_registry(tmp_path):
    """Test that no stub is generated when there's no SQLRegistry."""
    src = tmp_path / "plain.py"
    src.write_text("""
def sql(query):
    pass
sql("SELECT 1")
""")
    gen = TypeStubGenerator()
    stub_path = tmp_path / "plain.pyi"
    result = gen.generate_stub_file(src, stub_path)
    stub = stub_path.read_text()
    # Should not generate any Literal overloads (no SQLRegistry-based stubs)
    assert "Literal[" not in stub
    # Fallback overload should always be present
    assert "def sql" in stub
def test_invalid_syntax(tmp_path):
    src = tmp_path / "bad.py"
    src.write_text("def sql(:\n")
    gen = TypeStubGenerator()
    stub_path = tmp_path / "bad.pyi"
    # Should not raise, just print warning and produce minimal stub
    gen.generate_stub_file(src, stub_path)
    stub = stub_path.read_text()
    assert "class Query(" in stub
    assert "def sql" in stub
