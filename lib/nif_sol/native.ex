defmodule NifSol.Native do
  use Rustler, otp_app: :nif_sol, crate: :sol

  def add(a,b), do: error()
  def send_initialize_tx, do: error()

  defp error, do: :erlang.nif_error(:nif_not_loaded)
end
