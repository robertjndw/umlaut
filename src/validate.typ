// Argument checks. Anything that gets past these is valid input, so plugin errors are
// always about the diagram itself (broken XML, PlantUML syntax errors).

// `alt` names the extra value a parameter accepts besides `expected`, e.g. "auto".
#let fail(name, expected, got, alt: none) = panic(
  "umlaut: `" + name + "` must be " + expected + if alt != none { " or " + alt } + ", got " + repr(got),
)

// Diagram sources arrive as text (`read("a.puml")`) or bytes (`read("a.drawio", encoding: none)`).
#let source(value) = {
  if type(value) == str { bytes(value) }
  else if type(value) == bytes { value }
  else { fail("source", "a string or bytes (use read(\"file\") to load a file)", value) }
}

// A page index or, for multi-page draw.io files, the page's name.
#let page(value) = {
  if type(value) == int and value >= 0 { value }
  else if type(value) == str { value }
  else { fail("page", "a non-negative integer or a page name", value) }
}

// Sizes are a length, a ratio of the available space or a mix of both (`100% - 2cm`).
// Lengths may mix pt and em and ratios depend on the container, so whether the result
// is positive can only be decided where it is resolved (`render.resolve-size`); this
// only turns away wrong types and values that are not positive whatever they resolve to.
#let size(name, value) = {
  if value == auto { return auto }
  let t = type(value)
  if t != length and t != ratio and t != relative {
    fail(name, "a positive length or ratio", value, alt: "auto")
  }
  let part = if t == ratio { 0pt } else if t == relative { value.length } else { value }
  let share = if t == ratio { value } else if t == relative { value.ratio } else { 0% }
  if part.abs <= 0pt and part.em <= 0 and share <= 0% {
    fail(name, "a positive length or ratio", value, alt: "auto")
  }
  value
}

#let scale(value) = {
  if type(value) != ratio or value <= 0% { fail("scale", "a positive ratio such as 100%", value) }
  value / 100%
}

// `auto` keeps each text run's own font; a name or list of names overrides all text.
#let font(value) = {
  if value == auto { return auto }
  if type(value) == str { return (value,) }
  if type(value) == array and value.len() > 0 and value.all(f => type(f) == str) { return value }
  fail("font", "a font name or an array of names", value, alt: "auto")
}

// `warn` is a switch with a third state that turns warnings into errors.
#let warn(value) = {
  if type(value) != bool and value != "error" { fail("warn", "a boolean", value, alt: "\"error\"") }
  value
}

#let boolean(name, value) = {
  if type(value) != bool { fail(name, "a boolean", value) }
  value
}
