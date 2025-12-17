nutag revision="@":
  GITHUB_TOKEN=$(gh auth token) nutag -r {{revision}} --no-sign

dispatch-release version:
  gh workflow run update-formula.yml -r {{version}} -f tag={{version}}
