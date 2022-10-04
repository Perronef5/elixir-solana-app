defmodule NifSol do
  def add(a, b) when is_number(a) and is_number(b) do
    NifSol.Native.add(a, b)
  end

  def send_initialize_tx do
    NifSol.Native.send_initialize_tx()
  end
end
