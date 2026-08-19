//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using System;
using System.Collections.Generic;
using System.Linq;

namespace MonoFN.Cecil.Rocks
{
#if UNITY_EDITOR
    public
#endif
        static class ModuleDefinitionRocks
    {
        public static IEnumerable<TypeDefinition> GetAllTypes(this ModuleDefinition self)
        {
            if (self == null)
                throw new ArgumentNullException("self");

            // it was fun to write, but we need a somewhat less convoluted implementation
            return self.Types.SelectMany(Functional.Y<TypeDefinition, IEnumerable<TypeDefinition>>(f => type => type.NestedTypes.SelectMany(f).Prepend(type)));
        }
    }
}