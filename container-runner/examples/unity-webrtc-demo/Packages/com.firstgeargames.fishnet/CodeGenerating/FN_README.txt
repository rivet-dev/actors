After updating a custom Cecil to fix conflict with Unity.Burst in 2021 perform the following:

- Open cecil in it's own project; eg: do not place directly in FN.
- Rename namespace.Mono to namespace.MonoFN.
- Current project rename strings, "Mono   to   "MonoFN
- Replace current project #if INSIDE_ROCKS  to  #if UNITY_EDITOR
- Comment out `[assembly: AssemblyTitle ("MonoFN.Cecil.Rocks")]` within rocks\Mono.Cecil.Rocks\AssemblyInfo.cs.
- Delete obj/bin/tests folders.
- Copy into FN project.
