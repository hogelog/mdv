# frozen_string_literal: true

require_relative "lib/mdv/version"

Gem::Specification.new do |spec|
  spec.name = "mdv"
  spec.version = MDV::VERSION
  spec.authors = ["hogelog"]
  spec.summary = "A dependency-free local Markdown viewer"
  spec.description = "A local Markdown viewer with a background server bound to 127.0.0.1."
  spec.homepage = "https://github.com/hogelog/mdv"
  spec.license = "MIT"
  spec.required_ruby_version = ">= 3.2"
  spec.files = %w[LICENSE README.md exe/mdv lib/mdv.rb lib/mdv/version.rb]
  spec.bindir = "exe"
  spec.executables = ["mdv"]
  spec.require_paths = ["lib"]
  spec.add_dependency "commonmarker"
  spec.add_dependency "base64"
  spec.add_dependency "webrick"
end
