pub fn get_init_config_file_text() -> &'static str {
    r#"{
  "projectType": "", // required. Possible options according to your conscience: openSource, commercialSponsored, commercialDidNotSponsor
  "typescript": {},
  "json": {},
  "includes": ["**/*.{ts,tsx,js,jsx,json}"],
  "excludes": []
}
"#
}
