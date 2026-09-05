require_relative "../lib/mdv"

begin
  MDV.main(ARGV)
rescue StandardError => error
  warn "mdv: #{error.message}"
  exit 1
end
