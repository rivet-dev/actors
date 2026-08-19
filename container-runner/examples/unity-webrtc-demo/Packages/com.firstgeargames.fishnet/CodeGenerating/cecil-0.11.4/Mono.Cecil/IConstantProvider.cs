//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

namespace MonoFN.Cecil
{
    public interface IConstantProvider : IMetadataTokenProvider
    {
        bool HasConstant { get; set; }
        object Constant { get; set; }
    }

    internal static partial class Mixin
    {
        internal static object NoValue = new();
        internal static object NotResolved = new();

        public static void ResolveConstant(this IConstantProvider self, ref object constant, ModuleDefinition module)
        {
            if (module == null)
            {
                constant = NoValue;
                return;
            }

            lock (module.SyncRoot)
            {
                if (constant != NotResolved)
                    return;
                if (module.HasImage())
                    constant = module.Read(self, (provider, reader) => reader.ReadConstant(provider));
                else
                    constant = NoValue;
            }
        }
    }
}