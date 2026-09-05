# frozen_string_literal: true

require_relative "lib/mdv/version"

desc "Run tests"
task :test do
  ruby "-Ilib", "test/mdv_test.rb"
end

desc "Build the gem"
task :build do
  sh "gem build mdv.gemspec"
end

desc "Build and install the gem"
task install: :build do
  sh "gem install ./mdv-#{MDV::VERSION}.gem"
end

task default: :test
