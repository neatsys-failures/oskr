============================================
Meta-instruction on generating this document
============================================

The document is built in two steps: 

* Use Doxygen to extract code annotations from source.
* Use Sphinx to extend layout stubs (with Breathe extension) with Doxygen's 
  output, merge with out-of-source documents (like this one), and build final 
  bundle.

For Sphinx, install pypi packages ``sphinx``, ``furo`` as Sphinx theme, and 
``m2r2`` as markdown convertor.

For Doxygen and Breathe, they have to be built from source, because they only
support C++20 features in this project very recently. The following apt packages
are required: 

.. code-block:: text

    libllvm-13-dev libclang-13-dev flex bison
    
Source of Doxygen and Breathe are submodule of this project located in 
``dependency``.

Go to Breathe's source directory, install it as normal pypi package.

Use ``build`` as CMake building root and run

.. code-block:: bash

    make doc

The built document (identical to gh-pages branch content) is located in 
``build/doc/html``.
