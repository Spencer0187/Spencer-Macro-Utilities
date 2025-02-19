import React from 'react';
import styled, { createGlobalStyle } from 'styled-components';
import { Helmet, HelmetProvider } from 'react-helmet-async'; // Updated import

const GlobalStyle = createGlobalStyle`
  :root {
    --bg: linear-gradient(45deg, #0a0a0a 0%, #1a1a1a 100%);
    --card-bg: rgba(30, 30, 30, 0.95);
    --text: #e0e0e0;
    --accent: #2b7a78;
    --hover: #3daaaa;
    --section-spacing: 4rem;
  }

  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  body {
    background: var(--bg);
    color: var(--text);
    line-height: 1.6;
    font-family: 'Inter', system-ui, sans-serif;
    min-height: 100vh;
  }

  a {
    color: var(--hover);
    text-decoration: none;
    transition: opacity 0.2s ease;

    &:hover {
      opacity: 0.8;
      text-decoration: underline;
    }
  }
`;

const Container = styled.div`
  max-width: 1400px;
  margin: 0 auto;
  padding: 4rem 2rem;
`;

const Header = styled.header`
  text-align: center;
  margin-bottom: 6rem;
  position: relative;

  h1 {
    color: var(--hover);
    font-size: 3rem;
    margin-bottom: 1.5rem;
    letter-spacing: -0.05em;
  }

  p {
    max-width: 600px;
    margin: 0 auto;
    font-size: 1.1rem;
    opacity: 0.9;
  }
`;

const ButtonGroup = styled.div`
  display: flex;
  gap: 1.5rem;
  justify-content: center;
  margin: 3rem 0;

  @media (max-width: 768px) {
    flex-direction: column;
    align-items: center;
  }
`;

const Button = styled.a`
  padding: 1rem 2rem;
  border-radius: 8px;
  font-weight: 600;
  transition: transform 0.2s ease, box-shadow 0.2s ease;
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;

  &.primary {
    background: var(--accent);
    color: white;
    box-shadow: 0 4px 6px rgba(0, 0, 0, 0.1);

    &:hover {
      transform: translateY(-2px);
      box-shadow: 0 6px 12px rgba(43, 122, 120, 0.3);
    }
  }

  &.secondary {
    border: 2px solid var(--accent);
    color: var(--accent);

    &:hover {
      background: rgba(43, 122, 120, 0.1);
    }
  }
`;

const Section = styled.section`
  background: var(--card-bg);
  border-radius: 16px;
  padding: 3rem;
  margin-bottom: var(--section-spacing);
  backdrop-filter: blur(8px);
  border: 1px solid rgba(255, 255, 255, 0.1);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.1);

  h2 {
    color: var(--hover);
    margin-bottom: 2rem;
    font-size: 2rem;
    position: relative;
    padding-bottom: 1rem;

    &::after {
      content: '';
      position: absolute;
      bottom: 0;
      left: 0;
      width: 60px;
      height: 3px;
      background: var(--accent);
      border-radius: 2px;
    }
  }
`;

const FeatureGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
  gap: 2rem;
  margin-top: 2rem;
`;

const Feature = styled.div`
  padding: 2rem;
  background: rgba(0, 0, 0, 0.3);
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.1);

  h3 {
    margin-bottom: 1rem;
    color: var(--hover);
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  ul {
    padding-left: 1.5rem;
  }

  li {
    margin-bottom: 0.75rem;
    line-height: 1.5;
  }
`;

const ScreenshotGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 2rem;
  margin-top: 3rem;

  img {
    width: 100%;
    border-radius: 12px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 4px 6px rgba(0, 0, 0, 0.1);
    transition: transform 0.2s ease;

    &:hover {
      transform: translateY(-4px);
    }
  }
`;

const FullFeatureList = styled.div`
  columns: 2;
  gap: 2rem;
  margin-top: 2rem;

  @media (max-width: 768px) {
    columns: 1;
  }

  ul {
    list-style: none;
    padding: 0;
  }

  li {
    margin-bottom: 0.75rem;
    padding-left: 1.5rem;
    position: relative;

    &::before {
      content: '•';
      position: absolute;
      left: 0;
      color: var(--accent);
    }
  }
`;

const Footer = styled.footer`
  margin-top: var(--section-spacing);
  text-align: center;
  padding: 2rem;
  border-top: 1px solid rgba(255, 255, 255, 0.1);

  p {
    margin: 1rem 0;
    opacity: 0.8;
  }
`;

const App = () => {
  return (
    <HelmetProvider> {/* Wrap the app with HelmetProvider */}
      <Helmet>
        <meta name="google-site-verification" content="yuUkuubLUS0OYlP0U0Oi73OaOVmL-961B3KeujN8yHM" />
      </Helmet>
      <GlobalStyle />
      <Container>
        <Header>
          <h1>Roblox Macro Utilities</h1>
          <p>Open-source Windows automation tool with customizable interface and portable execution</p>
          
          <ButtonGroup>
            <Button 
              href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" 
              className="primary"
            >
              Download Portable EXE
            </Button>
            <Button 
              href="https://github.com/Spencer0187/Roblox-Macro-Utilities" 
              className="secondary"
            >
              View Source Code
            </Button>
          </ButtonGroup>
        </Header>

        {/* Rest of your components */}
      </Container>
    </HelmetProvider>
  );
};

export default App;
