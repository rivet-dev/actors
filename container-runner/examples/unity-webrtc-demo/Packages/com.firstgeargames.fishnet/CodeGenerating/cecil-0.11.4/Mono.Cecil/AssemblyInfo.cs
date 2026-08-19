//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Cecil;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

[assembly: AssemblyTitle(Consts.AssemblyName)]
[assembly: Guid("fd225bb4-fa53-44b2-a6db-85f5e48dcb54")]
[assembly: InternalsVisibleTo("MonoFN.Cecil.Tests, PublicKey=" + Consts.PublicKey)]
[assembly: InternalsVisibleTo("MonoFN.Cecil.Pdb, PublicKey=" + Consts.PublicKey)]
[assembly: InternalsVisibleTo("MonoFN.Cecil.Mdb, PublicKey=" + Consts.PublicKey)]
[assembly: InternalsVisibleTo("MonoFN.Cecil.Rocks, PublicKey=" + Consts.PublicKey)]