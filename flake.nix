{
  description = "monero_wallet_cli — the headless approver for monero_wallet_backend, driven over logosctl.";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    monero_wallet_backend = {
      url = "path:/Users/dlipicar/repos/logos-monero-wallet-backend";
      inputs.logos-module-builder.follows = "logos-module-builder";
    };
  };

  outputs = inputs@{ self, logos-module-builder, ... }:
    let
      nixpkgs = logos-module-builder.inputs.nixpkgs;
      systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
      targets = systems ++ [ "x86_64-windows" ];
    in
    {
      packages = nixpkgs.lib.genAttrs targets (system:
        (logos-module-builder.lib.mkLogosModule {
          src = ./.;
          configFile = ./metadata.json;
          flakeInputs = inputs;
        }).packages.${system});
    };
}
